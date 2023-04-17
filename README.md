# What

This is a small crate enabling viewport support for [imgui-rs](https://github.com/imgui-rs/imgui-rs) using the [imgui-wgpu-rs](https://github.com/Yatekii/imgui-wgpu-rs) renderer an the [winit](https://github.com/rust-windowing/winit) backend.

This crate is provided as is! I assembled this in a really short amount of time and it does work in a general sense but there will be errors bugs and crashes. For example it is known that resizing windows that are its own seperate viewport crashes wgpu. Feel free to open pull request wit fixes and improvements or open issues with problems or ideas you run into. But this does not mena that I will regulary work on this repo.

An example of how to use the crate is provided under ``examples/sample.rs``

## Dependencies

- imgui (With the docking branch)
- imgui-wgpu (Currently direct from github for the purpose of using imgui-rs version 0.11)
- winit
- env_logger
- wgpu
- pollster
- raw-window-handle