// mod advanced_text_input;
// mod run_command;
// mod todos;
// mod utils;
// mod wm_hints;
// mod xdg_manager;

// use std::sync::Arc;
// use std::thread;
// use std::time::Duration;

// use clap::Parser;

// use iced_core::{window, Pixels};
// use iced_wgpu::graphics::Viewport;
// use iced_wgpu::wgpu::rwh::HasWindowHandle;
// use iced_wgpu::{wgpu, Engine, Renderer};
// use iced_widget::runtime::program;
// use iced_winit::core::mouse;
// use iced_winit::core::renderer;
// use iced_winit::core::{Color, Size};
// use iced_winit::runtime::Debug;
// use iced_winit::winit::dpi::LogicalSize;
// use iced_winit::winit::keyboard::ModifiersState;
// use iced_winit::winit::platform::x11::{WindowAttributesExtX11, WindowType};
// use iced_winit::{conversion, futures, winit, Clipboard, Proxy};

// use signal_hook::consts::SIGUSR1;
// use signal_hook::iterator::Signals;

// use winit::{
//     event::{Event, WindowEvent},
//     event_loop::EventLoopBuilder,
// };

// use todos::Todos;

// pub static APP_NAME: &'static str = "statusbar";

// #[derive(Parser, Debug, Clone)]
// #[clap(about, version, author)]
// struct Opts {
//     #[clap(long)]
//     nocache: bool,
//     #[clap(long)]
//     remote_url: Option<String>,
// }

// pub fn main() -> Result<(), Box<dyn std::error::Error>> {
//     env_logger::init();

//     // parse arguments
//     let Opts {
//         nocache,
//         remote_url,
//     } = Opts::parse();

//     // Initialize winit
//     let event_loop = EventLoopBuilder::with_user_event().build()?;

//     let window = Arc::new(
//         WindowAttributesExtX11::new()
//             .with_x11_window_type(vec![WindowType::Dock])
//             .with_inner_size(LogicalSize::new(1, 50))
//             .build(&event_loop)?,
//     );

//     let window_id = window::Id::unique();

//     let physical_size = window.inner_size();
//     let mut viewport = Viewport::with_physical_size(
//         Size::new(physical_size.width, physical_size.height),
//         window.scale_factor(),
//     );
//     let mut cursor_position = None;
//     let mut modifiers = ModifiersState::default();
//     let mut clipboard = Clipboard::connect(&window);

//     // create runtime
//     let mut runtime = {
//         let proxy = Proxy::new(event_loop.create_proxy());
//         let executor = tokio::runtime::Runtime::new().unwrap();
//         iced_futures::Runtime::new(executor, proxy)
//     };

//     // Initialize wgpu
//     let default_backend = wgpu::Backends::PRIMARY;

//     let backend = wgpu::util::backend_bits_from_env().unwrap_or(default_backend);

//     let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
//         backends: backend,
//         ..Default::default()
//     });

//     let surface = instance.create_surface(window.clone())?;

//     let (format, device, queue, adapter) = futures::futures::executor::block_on(async {
//         let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface))
//             .await
//             .expect("Create adapter");

//         let capabilities = surface.get_capabilities(&adapter);

//         let format = capabilities
//             .formats
//             .iter()
//             .copied()
//             .find(wgpu::TextureFormat::is_srgb)
//             .or_else(|| capabilities.formats.first().copied())
//             .expect("Get preferred format");

//         let (device, queue) = adapter
//             .request_device(&wgpu::DeviceDescriptor::default(), None)
//             .await
//             .expect("Request device");

//         (format, device, queue, adapter)
//     });

//     fn get_surface_configuration(
//         size: winit::dpi::PhysicalSize<u32>,
//     ) -> wgpu::SurfaceConfiguration {
//         wgpu::SurfaceConfiguration {
//             usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
//             format: wgpu::TextureFormat::Bgra8UnormSrgb,
//             width: size.width,
//             height: size.height,
//             present_mode: wgpu::PresentMode::AutoVsync,
//             alpha_mode: wgpu::CompositeAlphaMode::Opaque,
//             view_formats: vec![],
//             desired_maximum_frame_latency: 1,
//         }
//     }

//     surface.configure(&device, &get_surface_configuration(physical_size));

//     let mut resized = false;

//     // Initialize scene and GUI controls
//     let wm_state_mgr = wm_hints::create_state_mgr(window.clone().as_ref()).unwrap();
//     // initialize app state
//     let todos = Todos::new(window_id, wm_state_mgr, nocache, remote_url).unwrap();

//     // Initialize iced
//     let mut debug = Debug::new();
//     let font = iced_core::Font::default();
//     let mut renderer = Renderer::new(
//         &device,
//         &Engine::new(&adapter, &device, &queue, format, None),
//         font,
//         Pixels(16.0),
//     );

//     let mut state = program::State::new(todos, viewport.logical_size(), &mut renderer, &mut debug);

//     // start catching the SIGUSR1 (open and focus) signal
//     let signal_proxy = event_loop.create_proxy();
//     let mut signals = Signals::new(&[SIGUSR1])?;

//     thread::spawn(move || {
//         for sig in signals.forever() {
//             match sig {
//                 SIGUSR1 => {
//                     let _ = signal_proxy.send_event(todos::Message::ExpandDock);
//                     // debounce the enter key (which is annoying sometimes)
//                     thread::sleep(Duration::from_millis(100));
//                     let _ = signal_proxy.send_event(todos::Message::FocusDock);
//                 }
//                 _ => (),
//             }
//         }
//     });

//     // Run event loop
//     event_loop.run(move |event, event_loop| {
//         match event {
//             Event::UserEvent(message) => {
//                 // handle events that come in from completed futures
//                 state.queue_message(message);
//                 window.request_redraw();
//             }
//             Event::WindowEvent { event, .. } => {
//                 match event {
//                     WindowEvent::CursorMoved { position, .. } => {
//                         cursor_position = Some(position);
//                     }
//                     WindowEvent::ModifiersChanged(new_modifiers) => {
//                         modifiers = new_modifiers.state();
//                     }
//                     WindowEvent::Resized(size) => {
//                         viewport = Viewport::with_physical_size(
//                             Size::new(size.width, size.height),
//                             window.scale_factor(),
//                         );

//                         resized = true;
//                     }
//                     WindowEvent::CloseRequested => {
//                         event_loop.exit();
//                     }
//                     WindowEvent::RedrawRequested => {
//                         // If there are events pending
//                         while !state.is_queue_empty() {
//                             // We update iced
//                             let (unhandled_events, command) = state.update(
//                                 viewport.logical_size(),
//                                 cursor_position
//                                     .map(|p| {
//                                         conversion::cursor_position(p, viewport.scale_factor())
//                                     })
//                                     .map(mouse::Cursor::Available)
//                                     .unwrap_or(mouse::Cursor::Unavailable),
//                                 &mut renderer,
//                                 &Theme::Dark,
//                                 &renderer::Style {
//                                     text_color: Color::WHITE,
//                                 },
//                                 &mut clipboard,
//                                 &mut debug,
//                             );

//                             // handle uncaptured events
//                             for e in unhandled_events {
//                                 state.queue_message(state.program().handle_uncaptured_event(e))
//                             }

//                             if let Some(command) = command {
//                                 run_command::run_command(
//                                     &mut state,
//                                     viewport.logical_size(),
//                                     &mut renderer,
//                                     command,
//                                     &mut runtime,
//                                     &mut clipboard,
//                                     &mut debug,
//                                     &window.as_ref(),
//                                 );
//                             }

//                             // and request a redraw
//                             if resized {
//                                 let size = window.inner_size();

//                                 viewport = Viewport::with_physical_size(
//                                     Size::new(size.width, size.height),
//                                     window.scale_factor(),
//                                 );

//                                 surface.configure(
//                                     &device,
//                                     &get_surface_configuration(winit::dpi::PhysicalSize::new(
//                                         size.width,
//                                         size.height,
//                                     )),
//                                 );

//                                 resized = false;
//                             }

//                             match surface.get_current_texture() {
//                                 Ok(frame) => {
//                                     let mut encoder = device.create_command_encoder(
//                                         &wgpu::CommandEncoderDescriptor { label: None },
//                                     );

//                                     let view = frame
//                                         .texture
//                                         .create_view(&wgpu::TextureViewDescriptor::default());

//                                     // And then iced on top
//                                     renderer.with_primitives(|backend, primitive| {
//                                         backend.present(
//                                             &device,
//                                             &queue,
//                                             &mut encoder,
//                                             None,
//                                             format,
//                                             &view,
//                                             primitive,
//                                             &viewport,
//                                             &debug.overlay(),
//                                         );
//                                     });

//                                     // Then we submit the work
//                                     queue.submit(Some(encoder.finish()));
//                                     frame.present();

//                                     // Update the mouse cursor
//                                     window.set_cursor_icon(
//                                         iced_winit::conversion::mouse_interaction(
//                                             state.mouse_interaction(),
//                                         ),
//                                     );
//                                 }
//                                 Err(error) => match error {
//                                     wgpu::SurfaceError::OutOfMemory => {
//                                         panic!(
//                                             "Swapchain error: {error}. \
//                                             Rendering cannot continue."
//                                         )
//                                     }
//                                     _ => {
//                                         // Try rendering again next frame.
//                                         window.request_redraw();
//                                     }
//                                 },
//                             }
//                         }
//                     }
//                     _ => {}
//                 }

//                 // Map window event to iced event
//                 if let Some(event) = iced_winit::conversion::window_event(
//                     window_id,
//                     event,
//                     window.scale_factor(),
//                     modifiers,
//                 ) {
//                     state.queue_event(event);
//                     window.request_redraw();
//                 }
//             }
//             _ => {}
//         }
//     })?;

//     Ok(())
// }

mod advanced_text_input;
mod run_command;
mod todos;
mod utils;
mod wm_hints;
mod xdg_manager;
mod scene;

use iced_style::Theme;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::{wgpu, Engine, Renderer};
use iced_winit::conversion;
use iced_winit::core::mouse;
use iced_winit::core::renderer;
use iced_winit::core::{Color, Font, Pixels, Size};
use iced_winit::futures;
use iced_winit::runtime::program;
use iced_winit::runtime::Debug;
use iced_winit::winit;
use iced_winit::Clipboard;

use scene::Scene;
use todos::Todos;
use winit::{
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
    keyboard::ModifiersState,
};

use std::sync::Arc;

pub static APP_NAME: &'static str = "statusbar";


pub fn main() -> Result<(), winit::error::EventLoopError> {
    // tracing_subscriber::fmt::init();

    // Initialize winit
    let event_loop = EventLoop::new()?;

    #[allow(clippy::large_enum_variant)]
    enum Runner {
        Loading,
        Ready {
            window: Arc<winit::window::Window>,
            device: wgpu::Device,
            queue: wgpu::Queue,
            surface: wgpu::Surface<'static>,
            format: wgpu::TextureFormat,
            engine: Engine,
            renderer: Renderer,
            scene: Scene,
            state: program::State<Todos>,
            cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
            clipboard: Clipboard,
            viewport: Viewport,
            modifiers: ModifiersState,
            resized: bool,
            debug: Debug,
        },
    }

    impl winit::application::ApplicationHandler for Runner {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            if let Self::Loading = self {
                let window = Arc::new(
                    event_loop
                        .create_window(
                            winit::window::WindowAttributes::default(),
                        )
                        .expect("Create window"),
                );

                let physical_size = window.inner_size();
                let viewport = Viewport::with_physical_size(
                    Size::new(physical_size.width, physical_size.height),
                    window.scale_factor(),
                );
                let clipboard = Clipboard::connect(window.clone());

                let backend =
                    wgpu::util::backend_bits_from_env().unwrap_or_default();

                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                    backends: backend,
                    ..Default::default()
                });
                let surface = instance
                    .create_surface(window.clone())
                    .expect("Create window surface");

                let (format, adapter, device, queue) =
                    futures::futures::executor::block_on(async {
                        let adapter =
                            wgpu::util::initialize_adapter_from_env_or_default(
                                &instance,
                                Some(&surface),
                            )
                            .await
                            .expect("Create adapter");

                        let adapter_features = adapter.features();

                        let capabilities = surface.get_capabilities(&adapter);

                        let (device, queue) = adapter
                            .request_device(
                                &wgpu::DeviceDescriptor {
                                    label: None,
                                    required_features: adapter_features
                                        & wgpu::Features::default(),
                                    required_limits: wgpu::Limits::default(),
                                },
                                None,
                            )
                            .await
                            .expect("Request device");

                        (
                            capabilities
                                .formats
                                .iter()
                                .copied()
                                .find(wgpu::TextureFormat::is_srgb)
                                .or_else(|| {
                                    capabilities.formats.first().copied()
                                })
                                .expect("Get preferred format"),
                            adapter,
                            device,
                            queue,
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
                        desired_maximum_frame_latency: 2,
                    },
                );

                // Initialize scene and GUI controls
                let scene = Scene::new(&device, format);
                let controls = Todos::new().unwrap();

                // Initialize iced
                let mut debug = Debug::new();
                let engine =
                    Engine::new(&adapter, &device, &queue, format, None);
                let mut renderer = Renderer::new(
                    &device,
                    &engine,
                    Font::default(),
                    Pixels::from(16),
                );

                let state = program::State::new(
                    controls,
                    viewport.logical_size(),
                    &mut renderer,
                    &mut debug,
                );

                // You should change this if you want to render continuously
                event_loop.set_control_flow(ControlFlow::Wait);

                *self = Self::Ready {
                    window,
                    device,
                    queue,
                    surface,
                    format,
                    engine,
                    renderer,
                    scene,
                    state,
                    cursor_position: None,
                    modifiers: ModifiersState::default(),
                    clipboard,
                    viewport,
                    resized: false,
                    debug,
                };
            }
        }

        fn window_event(
            &mut self,
            event_loop: &winit::event_loop::ActiveEventLoop,
            _window_id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            let Self::Ready {
                window,
                device,
                queue,
                surface,
                format,
                engine,
                renderer,
                scene,
                state,
                viewport,
                cursor_position,
                modifiers,
                clipboard,
                resized,
                debug,
            } = self
            else {
                return;
            };

            match event {
                WindowEvent::RedrawRequested => {
                    if *resized {
                        let size = window.inner_size();

                        *viewport = Viewport::with_physical_size(
                            Size::new(size.width, size.height),
                            window.scale_factor(),
                        );

                        surface.configure(
                            device,
                            &wgpu::SurfaceConfiguration {
                                format: *format,
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                width: size.width,
                                height: size.height,
                                present_mode: wgpu::PresentMode::AutoVsync,
                                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                                view_formats: vec![],
                                desired_maximum_frame_latency: 2,
                            },
                        );

                        *resized = false;
                    }

                    match surface.get_current_texture() {
                        Ok(frame) => {
                            let mut encoder = device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor { label: None },
                            );

                            let program = state.program();

                            let view = frame.texture.create_view(
                                &wgpu::TextureViewDescriptor::default(),
                            );

                            {
                                // We clear the frame
                                let mut render_pass = Scene::clear(
                                    &view,
                                    &mut encoder,
                                    program.background_color(),
                                );

                                // Draw the scene
                                // scene.draw(&mut render_pass);
                            }

                            // And then iced on top
                            renderer.present(
                                engine,
                                device,
                                queue,
                                &mut encoder,
                                None,
                                frame.texture.format(),
                                &view,
                                viewport,
                                &debug.overlay(),
                            );

                            // Then we submit the work
                            engine.submit(queue, encoder);
                            frame.present();

                            // Update the mouse cursor
                            window.set_cursor(
                                iced_winit::conversion::mouse_interaction(
                                    state.mouse_interaction(),
                                ),
                            );
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
                WindowEvent::CursorMoved { position, .. } => {
                    *cursor_position = Some(position);
                }
                WindowEvent::ModifiersChanged(new_modifiers) => {
                    *modifiers = new_modifiers.state();
                }
                WindowEvent::Resized(_) => {
                    *resized = true;
                }
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                _ => {}
            }

            // Map window event to iced event
            if let Some(event) = iced_winit::conversion::window_event(
                event,
                window.scale_factor(),
                *modifiers,
            ) {
                state.queue_event(event);
            }

            // If there are events pending
            if !state.is_queue_empty() {
                // We update iced
                let _ = state.update(
                    viewport.logical_size(),
                    cursor_position
                        .map(|p| {
                            conversion::cursor_position(
                                p,
                                viewport.scale_factor(),
                            )
                        })
                        .map(mouse::Cursor::Available)
                        .unwrap_or(mouse::Cursor::Unavailable),
                    renderer,
                    &Theme::Dark,
                    &renderer::Style {
                        text_color: Color::WHITE,
                    },
                    clipboard,
                    debug,
                );

                // and request a redraw
                window.request_redraw();
            }
        }
    }

    let mut runner = Runner::Loading;
    event_loop.run_app(&mut runner)
}