use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ptr::null_mut,
    rc::Rc,
};

use imgui::{ConfigFlags, Id, Key, MouseButton, ViewportFlags};
pub use imgui_wgpu::RendererConfig;

use imgui_wgpu::{Renderer as SRenderer, RendererError};
use raw_window_handle::HasRawWindowHandle;
use wgpu::{Surface, TextureFormat};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, KeyboardInput, TouchPhase, VirtualKeyCode},
    event_loop::EventLoopWindowTarget,
    window::{CursorIcon, WindowBuilder},
};

pub struct Renderer {
    main_renderer: SRenderer,
    extra_windows: HashMap<Id, (Option<SRenderer>, Surface, winit::window::Window)>,
    event_queue: Rc<RefCell<VecDeque<ViewportEvent>>>,
    last_cursor: CursorIcon,
}

struct ViewportData {
    pos: [f32; 2],
    size: [f32; 2],
    focus: bool,
    minimized: bool,
}
#[derive(Debug)]
enum ViewportEvent {
    Create(Id),
    Destroy(Id),
    SetPos(Id, [f32; 2]),
    SetSize(Id, [f32; 2]),
    SetVisible(Id),
    SetFocus(Id),
    SetTitle(Id, String),
}

struct PlatformBackend {
    event_queue: Rc<RefCell<VecDeque<ViewportEvent>>>,
}

impl Renderer {
    pub fn new(
        imgui: &mut imgui::Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        main_window: &winit::window::Window,
        renderer_config: RendererConfig,
    ) -> Self {
        let main_renderer = SRenderer::new(imgui, device, queue, renderer_config);

        match main_window.raw_window_handle() {
            raw_window_handle::RawWindowHandle::Wayland(_) => {}
            _ => {
                imgui
                    .io_mut()
                    .backend_flags
                    .insert(imgui::BackendFlags::PLATFORM_HAS_VIEWPORTS);
                imgui
                    .io_mut()
                    .backend_flags
                    .insert(imgui::BackendFlags::RENDERER_HAS_VIEWPORTS);
            }
        }

        imgui
            .io_mut()
            .backend_flags
            .insert(imgui::BackendFlags::HAS_MOUSE_CURSORS);
        imgui
            .io_mut()
            .backend_flags
            .insert(imgui::BackendFlags::HAS_SET_MOUSE_POS);

        imgui
            .io_mut()
            .backend_flags
            .insert(imgui::BackendFlags::RENDERER_HAS_VTX_OFFSET);

        let window_size = main_window.inner_size().cast::<f32>();
        imgui.io_mut().display_size = [window_size.width, window_size.height];
        imgui.io_mut().display_framebuffer_scale = [1.0, 1.0];

        let viewport = imgui.main_viewport_mut();

        let main_pos = main_window
            .inner_position()
            .unwrap_or_default()
            .cast::<f32>();

        viewport.pos = [main_pos.x, main_pos.y];
        viewport.work_pos = viewport.pos;
        viewport.size = [window_size.width, window_size.height];
        viewport.work_size = viewport.size;
        viewport.dpi_scale = 1.0;
        viewport.platform_user_data = Box::into_raw(Box::new(ViewportData {
            pos: [main_pos.x, main_pos.y],
            size: [window_size.width, window_size.height],
            focus: true,
            minimized: false,
        }))
        .cast();

        let mut monitors = Vec::new();
        for monitor in main_window.available_monitors() {
            monitors.push(imgui::PlatformMonitor {
                main_pos: [monitor.position().x as f32, monitor.position().y as f32],
                main_size: [monitor.size().width as f32, monitor.size().height as f32],
                work_pos: [monitor.position().x as f32, monitor.position().y as f32],
                work_size: [monitor.size().width as f32, monitor.size().height as f32],
                dpi_scale: 1.0,
            });
        }

        imgui
            .platform_io_mut()
            .monitors
            .replace_from_slice(&monitors);

        imgui.set_platform_name(Some(format!(
            "imgui-winit-wgpu-renderer-viewports {}",
            env!("CARGO_PKG_VERSION")
        )));
        imgui.set_renderer_name(Some(format!(
            "imgui-winit-wgpu-renderer-viewports {}",
            env!("CARGO_PKG_VERSION")
        )));

        let event_queue = Rc::new(RefCell::new(VecDeque::new()));

        imgui.set_platform_backend(PlatformBackend {
            event_queue: event_queue.clone(),
        });
        imgui.set_renderer_backend(RendererBackend {});

        Self {
            main_renderer,
            event_queue,
            extra_windows: HashMap::new(),
            last_cursor: CursorIcon::Default,
        }
    }

    pub fn handle_event<T>(
        &mut self,
        imgui: &mut imgui::Context,
        main_window: &winit::window::Window,
        device: &wgpu::Device,
        event: &winit::event::Event<T>,
    ) {
        match *event {
            winit::event::Event::WindowEvent {
                window_id,
                ref event,
            } => {
                let (window, viewport) = if window_id == main_window.id() {
                    (main_window, imgui.main_viewport_mut())
                } else if let Some((id, wnd)) =
                    self.extra_windows.iter().find_map(|(id, (_, _, wnd))| {
                        if wnd.id() == window_id {
                            Some((*id, wnd))
                        } else {
                            None
                        }
                    })
                {
                    if let Some(viewport) = imgui.viewport_by_id_mut(id) {
                        (wnd, viewport)
                    } else {
                        return;
                    }
                } else {
                    return;
                };

                match *event {
                    winit::event::WindowEvent::Resized(new_size) => {
                        unsafe {
                            (*(viewport.platform_user_data.cast::<ViewportData>())).size =
                                [new_size.width as f32, new_size.height as f32];
                        }

                        viewport.platform_request_resize = true;

                        if window_id == main_window.id() {
                            imgui.io_mut().display_size =
                                [new_size.width as f32, new_size.height as f32];
                        } else {
                            let surface_desc = wgpu::SurfaceConfiguration {
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                                width: window.inner_size().width,
                                height: window.inner_size().height,
                                present_mode: wgpu::PresentMode::Fifo,
                                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                                view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
                            };
                            let (_, surface, _) = self.extra_windows.get(&viewport.id).unwrap();

                            surface.configure(device, &surface_desc);
                        }
                    }
                    winit::event::WindowEvent::Moved(_) => unsafe {
                        let new_pos = window.inner_position().unwrap().cast::<f32>();
                        (*(viewport.platform_user_data.cast::<ViewportData>())).pos =
                            [new_pos.x, new_pos.y];

                        viewport.platform_request_move = true;
                    },
                    winit::event::WindowEvent::CloseRequested if window_id != main_window.id() => {
                        viewport.platform_request_close = true;
                    }
                    winit::event::WindowEvent::ReceivedCharacter(c) => {
                        imgui.io_mut().add_input_character(c);
                    }
                    winit::event::WindowEvent::Focused(f) => unsafe {
                        (*(viewport.platform_user_data.cast::<ViewportData>())).focus = f;
                    },
                    winit::event::WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(key),
                                state,
                                ..
                            },
                        ..
                    } => {
                        let pressed = state == ElementState::Pressed;

                        // We map both left and right ctrl to `ModCtrl`, etc.
                        // imgui is told both "left control is pressed" and
                        // "consider the control key is pressed". Allows
                        // applications to use either general "ctrl" or a
                        // specific key. Same applies to other modifiers.
                        // https://github.com/ocornut/imgui/issues/5047
                        handle_key_modifier(imgui.io_mut(), key, pressed);

                        // Add main key event
                        if let Some(key) = to_imgui_key(key) {
                            imgui.io_mut().add_key_event(key, pressed);
                        }
                    }
                    winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                        imgui
                            .io_mut()
                            .add_key_event(Key::ModShift, modifiers.shift());
                        imgui.io_mut().add_key_event(Key::ModCtrl, modifiers.ctrl());
                        imgui.io_mut().add_key_event(Key::ModAlt, modifiers.alt());
                        imgui
                            .io_mut()
                            .add_key_event(Key::ModSuper, modifiers.logo());
                    }
                    winit::event::WindowEvent::CursorMoved { position, .. } => {
                        if imgui
                            .io()
                            .config_flags
                            .contains(ConfigFlags::VIEWPORTS_ENABLE)
                        {
                            let window_pos =
                                window.inner_position().unwrap_or_default().cast::<f32>();
                            let pos = [
                                position.x as f32 + window_pos.x,
                                position.y as f32 + window_pos.y,
                            ];
                            imgui.io_mut().add_mouse_pos_event(pos);
                        } else {
                            imgui
                                .io_mut()
                                .add_mouse_pos_event([position.x as f32, position.y as f32]);
                        }
                    }
                    winit::event::WindowEvent::MouseWheel {
                        delta,
                        phase: TouchPhase::Moved,
                        ..
                    } => match delta {
                        winit::event::MouseScrollDelta::LineDelta(h, v) => {
                            imgui.io_mut().add_mouse_wheel_event([h, v]);
                        }
                        winit::event::MouseScrollDelta::PixelDelta(pos) => {
                            let h = if pos.x > 0.0 {
                                1.0
                            } else if pos.x < 0.0 {
                                -1.0
                            } else {
                                0.0
                            };
                            let v = if pos.y > 0.0 {
                                1.0
                            } else if pos.y < 0.0 {
                                -1.0
                            } else {
                                0.0
                            };
                            imgui.io_mut().add_mouse_wheel_event([h, v]);
                        }
                    },
                    winit::event::WindowEvent::MouseInput { state, button, .. } => {
                        let state = state == ElementState::Pressed;

                        if let Some(button) = to_imgui_mouse_button(button) {
                            imgui.io_mut().add_mouse_button_event(button, state);
                        }
                    }
                    _ => {}
                }
            }
            winit::event::Event::DeviceEvent {
                event:
                    DeviceEvent::Key(KeyboardInput {
                        virtual_keycode: Some(key),
                        state: ElementState::Released,
                        ..
                    }),
                ..
            } => {
                if let Some(key) = to_imgui_key(key) {
                    imgui.io_mut().add_key_event(key, false);
                }
            }
            _ => {}
        }
    }

    pub fn update_viewports<T>(
        &mut self,
        imgui: &mut imgui::Context,
        window_target: &EventLoopWindowTarget<T>,
        device: &wgpu::Device,
        instance: &wgpu::Instance,
    ) -> Result<(), RendererError> {
        loop {
            let event = self.event_queue.borrow_mut().pop_front();
            let event = if let Some(event) = event {
                event
            } else {
                break;
            };

            match event {
                ViewportEvent::Create(id) => {
                    if let Some(viewport) = imgui.viewport_by_id_mut(id) {
                        let extra_window =
                            self.create_extra_window(viewport, window_target, device, instance)?;
                        self.extra_windows.insert(id, extra_window);
                    }
                }
                ViewportEvent::Destroy(id) => {
                    self.extra_windows.remove(&id);
                }
                ViewportEvent::SetPos(id, pos) => {
                    if let Some((_, _, wnd)) = self.extra_windows.get(&id) {
                        wnd.set_outer_position(PhysicalPosition::new(pos[0], pos[1]));
                    }
                }
                ViewportEvent::SetSize(id, size) => {
                    if let Some((_, _, wnd)) = self.extra_windows.get(&id) {
                        wnd.set_inner_size(PhysicalSize::new(size[0], size[1]));
                    }
                }
                ViewportEvent::SetVisible(id) => {
                    if let Some((_, _, wnd)) = self.extra_windows.get(&id) {
                        wnd.set_visible(true);
                    }
                }
                ViewportEvent::SetFocus(id) => {
                    if let Some((_, _, wnd)) = self.extra_windows.get(&id) {
                        wnd.focus_window();
                    }
                }
                ViewportEvent::SetTitle(id, title) => {
                    if let Some((_, _, wnd)) = self.extra_windows.get(&id) {
                        wnd.set_title(&title);
                    }
                }
            }
        }

        Ok(())
    }

    fn create_extra_window<T>(
        &mut self,
        viewport: &mut imgui::Viewport,
        window_target: &EventLoopWindowTarget<T>,
        device: &wgpu::Device,
        instance: &wgpu::Instance,
    ) -> Result<(Option<SRenderer>, Surface, winit::window::Window), RendererError> {
        let window_builder = WindowBuilder::new()
            .with_position(PhysicalPosition::new(viewport.pos[0], viewport.pos[1]))
            .with_inner_size(PhysicalSize::new(viewport.size[0], viewport.size[1]))
            .with_visible(false)
            .with_resizable(true)
            .with_decorations(!viewport.flags.contains(ViewportFlags::NO_DECORATION));

        let window = window_builder.build(window_target).unwrap();

        let surface = unsafe { instance.create_surface(&window).unwrap() };

        let surface_desc = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: window.inner_size().width,
            height: window.inner_size().height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
        };

        surface.configure(device, &surface_desc);

        Ok((None, surface, window))
    }
    fn to_winit_cursor(cursor: imgui::MouseCursor) -> winit::window::CursorIcon {
        match cursor {
            imgui::MouseCursor::Arrow => winit::window::CursorIcon::Default,
            imgui::MouseCursor::TextInput => winit::window::CursorIcon::Text,
            imgui::MouseCursor::ResizeAll => winit::window::CursorIcon::Move,
            imgui::MouseCursor::ResizeNS => winit::window::CursorIcon::NsResize,
            imgui::MouseCursor::ResizeEW => winit::window::CursorIcon::EwResize,
            imgui::MouseCursor::ResizeNESW => winit::window::CursorIcon::NeswResize,
            imgui::MouseCursor::ResizeNWSE => winit::window::CursorIcon::NwseResize,
            imgui::MouseCursor::Hand => winit::window::CursorIcon::Hand,
            imgui::MouseCursor::NotAllowed => winit::window::CursorIcon::NotAllowed,
        }
    }

    pub fn render<'r>(
        &'r mut self,
        imgui: &mut imgui::Context,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
        rpass: &mut wgpu::RenderPass<'r>,
    ) -> imgui_wgpu::RendererResult<()> {
        self.main_renderer
            .render(imgui.render(), queue, device, rpass)?;

        Ok(())
    }

    pub fn render_viewports(
        &mut self,
        imgui: &mut imgui::Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_format: TextureFormat,
    ) {
        for (id, (renderer, surface, window)) in &mut self.extra_windows {
            if renderer.is_none() {
                *renderer = Some(SRenderer::new(
                    imgui,
                    device,
                    queue,
                    RendererConfig {
                        texture_format,
                        ..Default::default()
                    },
                ));
            }

            if let Some(viewport) = imgui.viewport_by_id(*id) {
                let draw_data = viewport.draw_data();

                let mut encoder: wgpu::CommandEncoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                let frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Dropped frame: {e:?}");
                        continue;
                    }
                };

                let size = frame.texture.size();
                let window_size = window.inner_size();
                if window_size.width != size.width && window_size.height != size.height {
                    let surface_desc = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        width: window_size.width,
                        height: window_size.height,
                        present_mode: wgpu::PresentMode::Fifo,
                        alpha_mode: wgpu::CompositeAlphaMode::Auto,
                        view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
                    };

                    surface.configure(device, &surface_desc);
                }

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            }),
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: None,
                });

                if let Some(renderer) = renderer {
                    renderer
                        .render(draw_data, queue, device, &mut rpass)
                        .expect("Failed render");
                }

                drop(rpass);

                queue.submit(Some(encoder.finish()));

                frame.present();
            }
        }
    }

    pub fn prepare_render(&mut self, imgui: &mut imgui::Context, window: &winit::window::Window) {
        if let Some(cursor) = imgui.mouse_cursor() {
            let cursor = Self::to_winit_cursor(cursor);

            if self.last_cursor != cursor {
                window.set_cursor_icon(cursor);

                for (_, _, wnd) in self.extra_windows.values() {
                    wnd.set_cursor_icon(cursor);
                }

                self.last_cursor = cursor;
            }
        }
    }
}

pub fn enable_docking_and_viewports(io: &mut imgui::Io, docking: bool, viewports: bool) {
    if docking {
        io.config_flags.insert(ConfigFlags::DOCKING_ENABLE);
    }
    if viewports {
        io.config_flags.insert(ConfigFlags::VIEWPORTS_ENABLE);
    }
}

impl imgui::PlatformViewportBackend for PlatformBackend {
    fn create_window(&mut self, viewport: &mut imgui::Viewport) {
        viewport.platform_user_data = Box::into_raw(Box::new(ViewportData {
            pos: viewport.pos,
            size: viewport.size,
            focus: false,
            minimized: false,
        }))
        .cast();
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::Create(viewport.id));
    }

    fn destroy_window(&mut self, viewport: &mut imgui::Viewport) {
        unsafe {
            drop(Box::from_raw(
                viewport.platform_user_data.cast::<ViewportData>(),
            ));
        }
        viewport.platform_user_data = null_mut();

        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::Destroy(viewport.id));
    }

    fn show_window(&mut self, viewport: &mut imgui::Viewport) {
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::SetVisible(viewport.id));
    }

    fn set_window_pos(&mut self, viewport: &mut imgui::Viewport, pos: [f32; 2]) {
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::SetPos(viewport.id, pos));
    }

    fn get_window_pos(&mut self, viewport: &mut imgui::Viewport) -> [f32; 2] {
        unsafe { (*(viewport.platform_user_data.cast::<ViewportData>())).pos }
    }

    fn set_window_size(&mut self, viewport: &mut imgui::Viewport, size: [f32; 2]) {
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::SetSize(viewport.id, size));
    }

    fn get_window_size(&mut self, viewport: &mut imgui::Viewport) -> [f32; 2] {
        unsafe { (*(viewport.platform_user_data.cast::<ViewportData>())).size }
    }

    fn set_window_focus(&mut self, viewport: &mut imgui::Viewport) {
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::SetFocus(viewport.id));
    }

    fn get_window_focus(&mut self, viewport: &mut imgui::Viewport) -> bool {
        unsafe { (*(viewport.platform_user_data.cast::<ViewportData>())).focus }
    }

    fn get_window_minimized(&mut self, viewport: &mut imgui::Viewport) -> bool {
        unsafe { (*(viewport.platform_user_data.cast::<ViewportData>())).minimized }
    }

    fn set_window_title(&mut self, viewport: &mut imgui::Viewport, title: &str) {
        self.event_queue
            .borrow_mut()
            .push_back(ViewportEvent::SetTitle(viewport.id, title.to_owned()));
    }

    fn set_window_alpha(&mut self, _viewport: &mut imgui::Viewport, _alpha: f32) {}

    fn update_window(&mut self, _viewport: &mut imgui::Viewport) {}

    fn render_window(&mut self, _viewport: &mut imgui::Viewport) {}

    fn swap_buffers(&mut self, _viewport: &mut imgui::Viewport) {}

    fn create_vk_surface(
        &mut self,
        _viewport: &mut imgui::Viewport,
        _instance: u64,
        _out_surface: &mut u64,
    ) -> i32 {
        0
    }
}

struct RendererBackend {}

impl imgui::RendererViewportBackend for RendererBackend {
    fn create_window(&mut self, _viewport: &mut imgui::Viewport) {}

    fn destroy_window(&mut self, _viewport: &mut imgui::Viewport) {}

    fn set_window_size(&mut self, _viewport: &mut imgui::Viewport, _size: [f32; 2]) {}

    fn render_window(&mut self, _viewport: &mut imgui::Viewport) {}

    fn swap_buffers(&mut self, _viewport: &mut imgui::Viewport) {}
}

fn handle_key_modifier(io: &mut imgui::Io, key: VirtualKeyCode, down: bool) {
    if key == VirtualKeyCode::LShift || key == VirtualKeyCode::RShift {
        io.add_key_event(imgui::Key::ModShift, down);
    } else if key == VirtualKeyCode::LControl || key == VirtualKeyCode::RControl {
        io.add_key_event(imgui::Key::ModCtrl, down);
    } else if key == VirtualKeyCode::LAlt || key == VirtualKeyCode::RAlt {
        io.add_key_event(imgui::Key::ModAlt, down);
    } else if key == VirtualKeyCode::LWin || key == VirtualKeyCode::RWin {
        io.add_key_event(imgui::Key::ModSuper, down);
    }
}

fn to_imgui_key(keycode: VirtualKeyCode) -> Option<Key> {
    match keycode {
        VirtualKeyCode::Tab => Some(Key::Tab),
        VirtualKeyCode::Left => Some(Key::LeftArrow),
        VirtualKeyCode::Right => Some(Key::RightArrow),
        VirtualKeyCode::Up => Some(Key::UpArrow),
        VirtualKeyCode::Down => Some(Key::DownArrow),
        VirtualKeyCode::PageUp => Some(Key::PageUp),
        VirtualKeyCode::PageDown => Some(Key::PageDown),
        VirtualKeyCode::Home => Some(Key::Home),
        VirtualKeyCode::End => Some(Key::End),
        VirtualKeyCode::Insert => Some(Key::Insert),
        VirtualKeyCode::Delete => Some(Key::Delete),
        VirtualKeyCode::Back => Some(Key::Backspace),
        VirtualKeyCode::Space => Some(Key::Space),
        VirtualKeyCode::Return => Some(Key::Enter),
        VirtualKeyCode::Escape => Some(Key::Escape),
        VirtualKeyCode::LControl => Some(Key::LeftCtrl),
        VirtualKeyCode::LShift => Some(Key::LeftShift),
        VirtualKeyCode::LAlt => Some(Key::LeftAlt),
        VirtualKeyCode::LWin => Some(Key::LeftSuper),
        VirtualKeyCode::RControl => Some(Key::RightCtrl),
        VirtualKeyCode::RShift => Some(Key::RightShift),
        VirtualKeyCode::RAlt => Some(Key::RightAlt),
        VirtualKeyCode::RWin => Some(Key::RightSuper),
        //VirtualKeyCode::Menu => Some(Key::Menu), // TODO: find out if there is a Menu key in winit
        VirtualKeyCode::Key0 => Some(Key::Alpha0),
        VirtualKeyCode::Key1 => Some(Key::Alpha1),
        VirtualKeyCode::Key2 => Some(Key::Alpha2),
        VirtualKeyCode::Key3 => Some(Key::Alpha3),
        VirtualKeyCode::Key4 => Some(Key::Alpha4),
        VirtualKeyCode::Key5 => Some(Key::Alpha5),
        VirtualKeyCode::Key6 => Some(Key::Alpha6),
        VirtualKeyCode::Key7 => Some(Key::Alpha7),
        VirtualKeyCode::Key8 => Some(Key::Alpha8),
        VirtualKeyCode::Key9 => Some(Key::Alpha9),
        VirtualKeyCode::A => Some(Key::A),
        VirtualKeyCode::B => Some(Key::B),
        VirtualKeyCode::C => Some(Key::C),
        VirtualKeyCode::D => Some(Key::D),
        VirtualKeyCode::E => Some(Key::E),
        VirtualKeyCode::F => Some(Key::F),
        VirtualKeyCode::G => Some(Key::G),
        VirtualKeyCode::H => Some(Key::H),
        VirtualKeyCode::I => Some(Key::I),
        VirtualKeyCode::J => Some(Key::J),
        VirtualKeyCode::K => Some(Key::K),
        VirtualKeyCode::L => Some(Key::L),
        VirtualKeyCode::M => Some(Key::M),
        VirtualKeyCode::N => Some(Key::N),
        VirtualKeyCode::O => Some(Key::O),
        VirtualKeyCode::P => Some(Key::P),
        VirtualKeyCode::Q => Some(Key::Q),
        VirtualKeyCode::R => Some(Key::R),
        VirtualKeyCode::S => Some(Key::S),
        VirtualKeyCode::T => Some(Key::T),
        VirtualKeyCode::U => Some(Key::U),
        VirtualKeyCode::V => Some(Key::V),
        VirtualKeyCode::W => Some(Key::W),
        VirtualKeyCode::X => Some(Key::X),
        VirtualKeyCode::Y => Some(Key::Y),
        VirtualKeyCode::Z => Some(Key::Z),
        VirtualKeyCode::F1 => Some(Key::F1),
        VirtualKeyCode::F2 => Some(Key::F2),
        VirtualKeyCode::F3 => Some(Key::F3),
        VirtualKeyCode::F4 => Some(Key::F4),
        VirtualKeyCode::F5 => Some(Key::F5),
        VirtualKeyCode::F6 => Some(Key::F6),
        VirtualKeyCode::F7 => Some(Key::F7),
        VirtualKeyCode::F8 => Some(Key::F8),
        VirtualKeyCode::F9 => Some(Key::F9),
        VirtualKeyCode::F10 => Some(Key::F10),
        VirtualKeyCode::F11 => Some(Key::F11),
        VirtualKeyCode::F12 => Some(Key::F12),
        VirtualKeyCode::Apostrophe => Some(Key::Apostrophe),
        VirtualKeyCode::Comma => Some(Key::Comma),
        VirtualKeyCode::Minus => Some(Key::Minus),
        VirtualKeyCode::Period => Some(Key::Period),
        VirtualKeyCode::Slash => Some(Key::Slash),
        VirtualKeyCode::Semicolon => Some(Key::Semicolon),
        VirtualKeyCode::Equals => Some(Key::Equal),
        VirtualKeyCode::LBracket => Some(Key::LeftBracket),
        VirtualKeyCode::Backslash => Some(Key::Backslash),
        VirtualKeyCode::RBracket => Some(Key::RightBracket),
        VirtualKeyCode::Grave => Some(Key::GraveAccent),
        VirtualKeyCode::Capital => Some(Key::CapsLock),
        VirtualKeyCode::Scroll => Some(Key::ScrollLock),
        VirtualKeyCode::Numlock => Some(Key::NumLock),
        VirtualKeyCode::Snapshot => Some(Key::PrintScreen),
        VirtualKeyCode::Pause => Some(Key::Pause),
        VirtualKeyCode::Numpad0 => Some(Key::Keypad0),
        VirtualKeyCode::Numpad1 => Some(Key::Keypad1),
        VirtualKeyCode::Numpad2 => Some(Key::Keypad2),
        VirtualKeyCode::Numpad3 => Some(Key::Keypad3),
        VirtualKeyCode::Numpad4 => Some(Key::Keypad4),
        VirtualKeyCode::Numpad5 => Some(Key::Keypad5),
        VirtualKeyCode::Numpad6 => Some(Key::Keypad6),
        VirtualKeyCode::Numpad7 => Some(Key::Keypad7),
        VirtualKeyCode::Numpad8 => Some(Key::Keypad8),
        VirtualKeyCode::Numpad9 => Some(Key::Keypad9),
        VirtualKeyCode::NumpadDecimal => Some(Key::KeypadDecimal),
        VirtualKeyCode::NumpadDivide => Some(Key::KeypadDivide),
        VirtualKeyCode::NumpadMultiply => Some(Key::KeypadMultiply),
        VirtualKeyCode::NumpadSubtract => Some(Key::KeypadSubtract),
        VirtualKeyCode::NumpadAdd => Some(Key::KeypadAdd),
        VirtualKeyCode::NumpadEnter => Some(Key::KeypadEnter),
        VirtualKeyCode::NumpadEquals => Some(Key::KeypadEqual),
        _ => None,
    }
}

fn to_imgui_mouse_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left | winit::event::MouseButton::Other(0) => {
            Some(imgui::MouseButton::Left)
        }
        winit::event::MouseButton::Right | winit::event::MouseButton::Other(1) => {
            Some(imgui::MouseButton::Right)
        }
        winit::event::MouseButton::Middle | winit::event::MouseButton::Other(2) => {
            Some(imgui::MouseButton::Middle)
        }
        winit::event::MouseButton::Other(3) => Some(imgui::MouseButton::Extra1),
        winit::event::MouseButton::Other(4) => Some(imgui::MouseButton::Extra2),
        _ => None,
    }
}
