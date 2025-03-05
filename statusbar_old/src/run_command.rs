use iced_futures::{Executor, Runtime};
use iced_wgpu::core::{Clipboard, Size};
use iced_widget::runtime::clipboard;
use iced_widget::runtime::program::State;
use iced_winit::runtime::{Command, Debug, Program};
use iced_winit::Proxy;
use iced_winit::{conversion, winit};

/// Runs the actions of a [`Command`].
pub fn run_command<P, E>(
    state: &mut State<P>,
    bounds: Size,
    renderer: &mut P::Renderer,
    command: Command<P::Message>,
    runtime: &mut Runtime<E, Proxy<P::Message>, P::Message>,
    clipboard: &mut dyn Clipboard,
    debug: &mut Debug,
    window: &winit::window::Window,
) where
    P: Program,
    E: Executor,
{
    use iced_widget::runtime::{command, window};

    // Iterate over each action in the command
    for action in command.actions() {
        match action {
            // Handle asynchronous futures by spawning them in the runtime
            command::Action::Future(future) => {
                runtime.spawn(future);
            }
            // Handle clipboard-related actions
            command::Action::Clipboard(action) => match action {
                // Read from the clipboard and queue a message with the result
                clipboard::Action::Read(tag, k) => {
                    state.queue_message(tag(clipboard.read(k)));
                }
                // Write to the clipboard
                clipboard::Action::Write(contents, kind) => {
                    clipboard.write(kind, contents);
                }
            },
            // Handle window-related actions
            command::Action::Window(action) => match action {
                // Drag the window
                window::Action::Drag(id) => {
                    let _res = window.drag_window();
                }
                // Resize the window to the specified size
                window::Action::Resize(id, size) => {
                    let _ = window.request_inner_size(winit::dpi::LogicalSize {
                        width: size.width,
                        height: size.height,
                    });
                }
                // Maximize or restore the window
                window::Action::Maximize(id, value) => {
                    window.set_maximized(value);
                }
                // Minimize or restore the window
                window::Action::Minimize(id, value) => {
                    window.set_minimized(value);
                }
                // Move the window to the specified position
                window::Action::Move(id, point) => {
                    window.set_outer_position(winit::dpi::PhysicalPosition {
                        x: point.x,
                        y: point.y,
                    });
                }
                // Change the window mode (e.g., fullscreen, windowed)
                window::Action::ChangeMode(id, mode) => {
                    window.set_visible(conversion::visible(mode));
                    window.set_fullscreen(conversion::fullscreen(window.primary_monitor(), mode));
                }
                // Fetch the current window mode and queue a message with the result
                window::Action::FetchMode(id, tag) => {
                    let mode = if window.is_visible().unwrap_or(true) {
                        conversion::mode(window.fullscreen())
                    } else {
                        iced_core::window::Mode::Hidden
                    };

                    state.queue_message(tag(mode));
                }
                // Toggle the window's maximized state
                window::Action::ToggleMaximize(id) => window.set_maximized(!window.is_maximized()),

                // Toggle window decorations (e.g., title bar, borders)
                window::Action::ToggleDecorations(id) => {
                    window.set_decorations(!window.is_decorated())
                }
                // Request user attention (e.g., flashing the taskbar icon)
                window::Action::RequestUserAttention(id, user_attention) => {
                    window.request_user_attention(user_attention.map(conversion::user_attention))
                }
                // Focus the window
                window::Action::GainFocus(id) => window.focus_window(),
                // Fetch the window ID and queue a message with the result
                window::Action::FetchId(id, tag) => {
                    state.queue_message(tag(window.id().into()));
                }
                // Change the window icon
                window::Action::ChangeIcon(id, icon) => {
                    window.set_window_icon(conversion::icon(icon))
                }
                // Fetch the current window size and queue a message with the result
                window::Action::FetchSize(id, callback) => {
                    let size = window.inner_size();
                    state.queue_message(callback(Size::new(size.width as f32, size.height as f32)))
                }
                // Change the window's level (e.g., always on top)
                window::Action::ChangeLevel(id, level) => {
                    window.set_window_level(conversion::window_level(level));
                }
                // Placeholder for screenshot functionality (not implemented)
                window::Action::Screenshot(id, _) => todo!(),
                // Placeholder for closing the window (not implemented)
                window::Action::Close(id) => todo!(),
                // Placeholder for spawning a new window (not implemented)
                window::Action::Spawn(_, _) => todo!(),
                // Placeholder for fetching the maximized state (not implemented)
                window::Action::FetchMaximized(_, _) => todo!(),
                // Placeholder for fetching the minimized state (not implemented)
                window::Action::FetchMinimized(_, _) => todo!(),
                // Placeholder for showing the system menu (not implemented)
                window::Action::ShowSystemMenu(_) => todo!(),
                // Placeholder for running actions with a window handle (not implemented)
                window::Action::RunWithHandle(_, _) => todo!(),
            },
            // Handle widget-related actions
            command::Action::Widget(action) => {
                state.operate(renderer, Some(action).into_iter(), bounds, debug);
            }
            // Placeholder for system-related actions (not implemented)
            command::Action::System(_) => todo!(),
            // Placeholder for loading fonts (not implemented)
            command::Action::LoadFont { .. } => todo!(),
            // Placeholder for handling streams (not implemented)
            command::Action::Stream(_) => todo!(),
            // Placeholder for custom actions (not implemented)
            command::Action::Custom(_) => todo!(),
        }
    }
}
