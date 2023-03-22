mod advanced_text_input;
mod program_runner;
mod todos;
mod utils;
mod wm_hints;
mod xdg_manager;

use clap::Parser;
use todos::Todos;

use iced_wgpu::{wgpu, Backend, Renderer, Settings, Viewport};
use iced_winit::{
    conversion, futures, renderer, winit, Clipboard, Color, Debug, Proxy, Runtime, Size,
};

use winit::{
    dpi::LogicalSize,
    dpi::PhysicalPosition,
    event::{Event, ModifiersState, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::unix::{WindowBuilderExtUnix, XWindowType},
};

pub static APP_NAME: &'static str = "statusbar";

#[derive(Parser, Debug, Clone)]
#[clap(about, version, author)]
struct Opts {
    #[clap(long)]
    nocache: bool,
    #[clap(long)]
    remote_url: Option<String>,
}

pub fn main() {
    env_logger::init();

    // parse arguments
    let Opts { nocache, remote_url, } = Opts::parse();

    // Initialize winit
    let event_loop =
        EventLoopBuilder::<<Todos as program_runner::ProgramWithSubscription>::Message>::with_user_event().build();

    let window = winit::window::WindowBuilder::new()
        .with_x11_window_type(vec![XWindowType::Dock])
        // todo: don't hardcode this, use an initial command or something
        .with_inner_size(LogicalSize::new(1, 50))
        .build(&event_loop)
        .unwrap();

    let physical_size = window.inner_size();

    let wm_state_mgr = wm_hints::create_state_mgr(&window).unwrap();
    // initialize app state
    let todos = Todos::new(wm_state_mgr, nocache, remote_url).unwrap();

    let mut viewport = Viewport::with_physical_size(
        Size::new(physical_size.width, physical_size.height),
        window.scale_factor(),
    );

    let mut cursor_position = PhysicalPosition::new(-1.0, -1.0);
    let mut modifiers = ModifiersState::default();
    let mut clipboard = Clipboard::connect(&window);
    let mut proxy = event_loop.create_proxy();
    let mut runtime = {
        let proxy = Proxy::new(proxy.clone());
        let executor = tokio::runtime::Runtime::new().unwrap();
        Runtime::new(executor, proxy)
    };

    // Initialize wgpu
    let backend = wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::PRIMARY);

    let instance = wgpu::Instance::new(backend);
    let surface = unsafe { instance.create_surface(&window) };

    let (format, (device, queue)) = futures::executor::block_on(async {
        let adapter =
            wgpu::util::initialize_adapter_from_env_or_default(&instance, backend, Some(&surface))
                .await
                .expect("No suitable GPU adapters found on the system!");

        let adapter_features = adapter.features();

        let needed_limits = wgpu::Limits::default();

        (
            surface
                .get_supported_formats(&adapter)
                .first()
                .copied()
                .expect("Get preferred format"),
            adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: None,
                        features: adapter_features & wgpu::Features::default(),
                        limits: needed_limits,
                    },
                    None,
                )
                .await
                .expect("Request device"),
        )
    });

    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        },
    );

    let mut need_to_resize_surface = false;

    // Initialize staging belt
    let mut staging_belt = wgpu::util::StagingBelt::new(5 * 1024);

    // Initialize iced
    let mut debug = Debug::new();
    let mut renderer = Renderer::new(Backend::new(&device, Settings::default(), format));

    let mut state =
        program_runner::State::new(todos, viewport.logical_size(), &mut renderer, &mut debug);

    // Run event loop
    event_loop.run(move |event, _, control_flow| {
        // You should change this if you want to render continuosly
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(message) => {
                state.queue_message(message);
            }
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CursorMoved { position, .. } => {
                        cursor_position = position;
                    }
                    WindowEvent::ModifiersChanged(new_modifiers) => {
                        modifiers = new_modifiers;
                    }
                    WindowEvent::Resized(size) => {
                        // change viewport
                        viewport = Viewport::with_physical_size(
                            Size::new(size.width, size.height),
                            window.scale_factor(),
                        );
                        // in the next frame we'll have to resize the surface
                        need_to_resize_surface = true;
                    }
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                }

                // Map window event to iced event
                if let Some(event) =
                    iced_winit::conversion::window_event(&event, window.scale_factor(), modifiers)
                {
                    state.queue_event(event);
                }
            }
            Event::MainEventsCleared => {
                // If there are events pending
                if !state.is_queue_empty() {
                    // We update iced
                    let maybe_command = state.update(
                        viewport.logical_size(),
                        conversion::cursor_position(cursor_position, viewport.scale_factor()),
                        &mut renderer,
                        &iced_wgpu::Theme::Dark,
                        &renderer::Style {
                            text_color: Color::WHITE,
                        },
                        &mut clipboard,
                        &mut debug,
                    );

                    // run the command that was gotten from iced
                    if let Some(command) = maybe_command {
                        state.run_command(
                            command,
                            viewport.logical_size(),
                            conversion::cursor_position(cursor_position, viewport.scale_factor()),
                            &mut renderer,
                            &iced_wgpu::Theme::Dark,
                            &renderer::Style {
                                text_color: Color::WHITE,
                            },
                            &mut runtime,
                            &mut clipboard,
                            &mut proxy,
                            &mut debug,
                            &window,
                        );
                    }

                    // and request a redraw
                    window.request_redraw();
                }
            }
            Event::RedrawRequested(_) => {
                if need_to_resize_surface {
                    let size = window.inner_size();

                    surface.configure(
                        &device,
                        &wgpu::SurfaceConfiguration {
                            format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            width: size.width,
                            height: size.height,
                            present_mode: wgpu::PresentMode::AutoVsync,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                        },
                    );

                    need_to_resize_surface = false;
                }

                match surface.get_current_texture() {
                    Ok(frame) => {
                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: None,
                            });

                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        // And then iced on top
                        renderer.with_primitives(|backend, primitive| {
                            backend.present(
                                &device,
                                &mut staging_belt,
                                &mut encoder,
                                &view,
                                primitive,
                                &viewport,
                                &debug.overlay(),
                            );
                        });

                        // Then we submit the work
                        staging_belt.finish();
                        queue.submit(Some(encoder.finish()));
                        frame.present();

                        // Update the mouse cursor
                        window.set_cursor_icon(iced_winit::conversion::mouse_interaction(
                            state.mouse_interaction(),
                        ));

                        // And recall staging buffers
                        staging_belt.recall();
                    }
                    Err(error) => match error {
                        wgpu::SurfaceError::OutOfMemory => {
                            panic!("Swapchain error: {}. Rendering cannot continue.", error)
                        }
                        _ => {
                            // Try rendering again next frame.
                            window.request_redraw();
                        }
                    },
                }
            }
            _ => {}
        }
    })
}
