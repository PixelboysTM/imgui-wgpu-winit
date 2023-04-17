use std::time::Instant;

use imgui::{Condition, ConfigFlags, FontSource};
use imgui_wgpu_winit::Renderer;
use pollster::block_on;
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

fn main() {
    println!("Hello, world!");

    let event_loop = EventLoop::new();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });

    let (window, size, surface) = {
        let version = env!("CARGO_PKG_VERSION");

        let window = Window::new(&event_loop).unwrap();
        window.set_inner_size(LogicalSize {
            width: 1280.0,
            height: 720.0,
        });
        window.set_title(&format!("imgui-wgpu {version}"));
        let size = window.inner_size();

        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        (window, size, surface)
    };

    let hidpi_factor = 1.0;
    window.scale_factor();

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();

    // Set up swap chain
    let surface_desc = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
    };

    surface.configure(&device, &surface_desc);

    // Set up dear imgui
    let mut imgui = imgui::Context::create();
    // let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui);
    // platform.attach_window(
    //     imgui.io_mut(),
    //     &window,
    //     imgui_winit_support::HiDpiMode::Default,
    // );
    imgui.set_ini_filename(None);

    let font_size = (13.0 * hidpi_factor) as f32;
    imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;

    imgui.fonts().add_font(&[FontSource::DefaultFontData {
        config: Some(imgui::FontConfig {
            oversample_h: 1,
            pixel_snap_h: true,
            size_pixels: font_size,
            ..Default::default()
        }),
    }]);

    imgui
        .io_mut()
        .config_flags
        .insert(ConfigFlags::DOCKING_ENABLE);

    imgui_wgpu_winit::enable_docking_and_viewports(imgui.io_mut(), true, true);

    //
    // Set up dear imgui wgpu renderer
    //
    let clear_color = wgpu::Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 1.0,
    };

    let renderer_config = imgui_wgpu_winit::RendererConfig {
        texture_format: surface_desc.format,
        ..Default::default()
    };

    let mut renderer = Renderer::new(&mut imgui, &device, &queue, &window, renderer_config);

    let mut last_frame = Instant::now();
    let mut demo_open = true;

    // let mut last_cursor = None;

    // Event loop
    event_loop.run(move |event, e_loop, control_flow| {
        *control_flow = if cfg!(feature = "metal-auto-capture") {
            ControlFlow::Exit
        } else {
            ControlFlow::Poll
        };

        renderer.handle_event(&mut imgui, &window, &device, &event);

        match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                window_id,
            } if window_id == window.id() => {
                let size = window.inner_size();

                let surface_desc = wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    width: size.width,
                    height: size.height,
                    present_mode: wgpu::PresentMode::Fifo,
                    alpha_mode: wgpu::CompositeAlphaMode::Auto,
                    view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
                };

                surface.configure(&device, &surface_desc);
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                state: ElementState::Pressed,
                                ..
                            },
                        ..
                    },
                ..
            }
            | Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                control_flow.set_exit();
            }
            Event::MainEventsCleared => window.request_redraw(),
            Event::RedrawEventsCleared => {
                let delta_s = last_frame.elapsed();
                let now = Instant::now();
                imgui.io_mut().update_delta_time(now - last_frame);
                last_frame = now;

                let frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("dropped frame: {e:?}");
                        return;
                    }
                };
                // platform
                //     .prepare_frame(imgui.io_mut(), &window)
                //     .expect("Failed to prepare frame");
                let ui = imgui.frame();

                {
                    ui.dockspace_over_main_viewport();

                    let window = ui.window("Hello world");
                    window
                        .size([300.0, 100.0], Condition::FirstUseEver)
                        .build(|| {
                            ui.text("Hello world!");
                            ui.text("This...is...imgui-rs on WGPU!");
                            ui.separator();
                            let mouse_pos = ui.io().mouse_pos;
                            ui.text(format!(
                                "Mouse Position: ({:.1},{:.1})",
                                mouse_pos[0], mouse_pos[1]
                            ));

                            ui.separator();
                            ui.text("Hello firdnsus\nadasdfg")
                        });

                    let window = ui.window("Hello too");
                    window
                        .size([400.0, 200.0], Condition::FirstUseEver)
                        .position([400.0, 200.0], Condition::FirstUseEver)
                        .build(|| {
                            ui.text(format!("Frametime: {delta_s:?}"));
                        });

                    ui.show_demo_window(&mut demo_open);
                }

                let mut encoder: wgpu::CommandEncoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                // if last_cursor != Some(ui.mouse_cursor()) {
                //     last_cursor = Some(ui.mouse_cursor());
                //     // platform.prepare_render(ui, &window);
                // }

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color),
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: None,
                });

                ui.end_frame_early();

                renderer.prepare_render(&mut imgui, &window);

                imgui.update_platform_windows();
                renderer
                    .update_viewports(&mut imgui, &e_loop, &device, &instance)
                    .expect("Failed to update viewports.");

                renderer
                    .render(&mut imgui, &queue, &device, &mut rpass)
                    .expect("Rendering failed");
                // renderer
                //     .render(imgui.render(), &queue, &device, &mut rpass)
                //     .expect("Rendering failed");

                drop(rpass);

                queue.submit(Some(encoder.finish()));

                let format = frame.texture.format();
                frame.present();

                renderer.render_viewports(&mut imgui, &device, &queue, format);
            }
            _ => (),
        }

        // platform.handle_event(imgui.io_mut(), &window, &event);
    });
}
