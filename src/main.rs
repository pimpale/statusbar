mod advanced_text_input;
mod run_command;
mod todos;
mod utils;
mod wm_hints;
mod xdg_manager;

use std::thread;
use std::time::Duration;

use clap::Parser;

use iced_wgpu::graphics::Viewport;
use iced_wgpu::{wgpu, Backend, Renderer, Settings};
use iced_widget::runtime::program;
use iced_winit::core::mouse;
use iced_winit::core::renderer;
use iced_winit::core::{Color, Size};
use iced_winit::runtime::Debug;
use iced_winit::style::Theme;
use iced_winit::winit::dpi::LogicalSize;
use iced_winit::{conversion, futures, winit, Clipboard, Proxy};

use signal_hook::consts::SIGUSR1;
use signal_hook::iterator::Signals;

use winit::{
    event::{Event, ModifiersState, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::x11::WindowBuilderExtX11,
    platform::x11::XWindowType,
};

use todos::Todos;

pub static APP_NAME: &'static str = "statusbar";

#[derive(Parser, Debug, Clone)]
#[clap(about, version, author)]
struct Opts {
    #[clap(long)]
    nocache: bool,
    #[clap(long)]
    remote_url: Option<String>,
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // parse arguments
    let Opts {
        nocache,
        remote_url,
    } = Opts::parse();

    // Initialize winit
    let event_loop = EventLoopBuilder::with_user_event().build();

    let window = winit::window::WindowBuilder::new()
        .with_x11_window_type(vec![XWindowType::Dock])
        .with_inner_size(LogicalSize::new(1, 50))
        .build(&event_loop)?;

    let physical_size = window.inner_size();
    let mut viewport = Viewport::with_physical_size(
        Size::new(physical_size.width, physical_size.height),
        window.scale_factor(),
    );
    let mut cursor_position = None;
    let mut modifiers = ModifiersState::default();
    let mut clipboard = Clipboard::connect(&window);

    // create runtime
    let mut runtime = {
        let proxy = Proxy::new(event_loop.create_proxy());
        let executor = tokio::runtime::Runtime::new().unwrap();
        iced_futures::Runtime::new(executor, proxy)
    };

    // Initialize wgpu
    let default_backend = wgpu::Backends::PRIMARY;

    let backend = wgpu::util::backend_bits_from_env().unwrap_or(default_backend);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: backend,
        ..Default::default()
    });

    let surface = unsafe { instance.create_surface(&window) }?;

    let (format, (device, queue)) = futures::futures::executor::block_on(async {
        let adapter =
            wgpu::util::initialize_adapter_from_env_or_default(&instance, backend, Some(&surface))
                .await
                .expect("Create adapter");

        let adapter_features = adapter.features();

        let capabilities = surface.get_capabilities(&adapter);

        (
            capabilities
                .formats
                .iter()
                .copied()
                .find(wgpu::TextureFormat::is_srgb)
                .or_else(|| capabilities.formats.first().copied())
                .expect("Get preferred format"),
            adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: None,
                        features: adapter_features & wgpu::Features::default(),
                        limits: wgpu::Limits::default(),
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
            view_formats: vec![],
        },
    );

    let mut resized = false;

    // Initialize scene and GUI controls
    let wm_state_mgr = wm_hints::create_state_mgr(&window).unwrap();
    // initialize app state
    let todos = Todos::new(wm_state_mgr, nocache, remote_url).unwrap();

    // Initialize iced
    let mut debug = Debug::new();
    let mut renderer = Renderer::new(Backend::new(&device, &queue, Settings::default(), format));

    let mut state = program::State::new(todos, viewport.logical_size(), &mut renderer, &mut debug);

    // start catching the SIGUSR1 (open and focus) signal
    let signal_proxy = event_loop.create_proxy();
    let mut signals = Signals::new(&[SIGUSR1])?;

    thread::spawn(move || {
        for sig in signals.forever() {
            match sig {
                SIGUSR1 => {
                    let _ = signal_proxy.send_event(todos::Message::ExpandDock);
                    // debounce the enter key (which is annoying sometimes)
                    thread::sleep(Duration::from_millis(100));
                    let _ = signal_proxy.send_event(todos::Message::FocusDock);
                }
                _ => (),
            }
        }
    });

    // Run event loop
    event_loop.run(move |event, _, control_flow| {
        // You should change this if you want to render continuosly
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(message) => {
                // handle events that come in from completed futures
                state.queue_message(message);
            }
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CursorMoved { position, .. } => {
                        cursor_position = Some(position);
                    }
                    WindowEvent::ModifiersChanged(new_modifiers) => {
                        modifiers = new_modifiers;
                    }
                    WindowEvent::Resized(size) => {
                        viewport = Viewport::with_physical_size(
                            Size::new(size.width, size.height),
                            window.scale_factor(),
                        );

                        resized = true;
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
                while !state.is_queue_empty() {
                    // We update iced
                    let (unhandled_events, command) = state.update(
                        viewport.logical_size(),
                        cursor_position
                            .map(|p| conversion::cursor_position(p, viewport.scale_factor()))
                            .map(mouse::Cursor::Available)
                            .unwrap_or(mouse::Cursor::Unavailable),
                        &mut renderer,
                        &Theme::Dark,
                        &renderer::Style {
                            text_color: Color::WHITE,
                        },
                        &mut clipboard,
                        &mut debug,
                    );

                    // handle uncaptured events
                    for e in unhandled_events {
                        state.queue_message(state.program().handle_uncaptured_event(e))
                    }

                    if let Some(command) = command {
                        run_command::run_command(
                            &mut state,
                            viewport.logical_size(),
                            &mut renderer,
                            command,
                            &mut runtime,
                            &mut clipboard,
                            &mut debug,
                            &window,
                        );
                    }

                    // and request a redraw
                    window.request_redraw();
                }
            }
            Event::RedrawRequested(_) => {
                if resized {
                    let size = window.inner_size();

                    viewport = Viewport::with_physical_size(
                        Size::new(size.width, size.height),
                        window.scale_factor(),
                    );

                    surface.configure(
                        &device,
                        &wgpu::SurfaceConfiguration {
                            format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            width: size.width,
                            height: size.height,
                            present_mode: wgpu::PresentMode::AutoVsync,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                            view_formats: vec![],
                        },
                    );

                    resized = false;
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
                                &queue,
                                &mut encoder,
                                None,
                                &view,
                                primitive,
                                &viewport,
                                &debug.overlay(),
                            );
                        });

                        // Then we submit the work
                        queue.submit(Some(encoder.finish()));
                        frame.present();

                        // Update the mouse cursor
                        window.set_cursor_icon(iced_winit::conversion::mouse_interaction(
                            state.mouse_interaction(),
                        ));
                    }
                    Err(error) => match error {
                        wgpu::SurfaceError::OutOfMemory => {
                            panic!(
                                "Swapchain error: {error}. \
                                Rendering cannot continue."
                            )
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
