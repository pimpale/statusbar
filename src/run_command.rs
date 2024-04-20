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

    for action in command.actions() {
        match action {
            command::Action::Future(future) => {
                runtime.spawn(future);
            }
            command::Action::Clipboard(action) => match action {
                clipboard::Action::Read(tag, k) => {
                    state.queue_message(tag(clipboard.read(k)));
                }
                clipboard::Action::Write(contents, kind) => {
                    clipboard.write(kind, contents);
                }
            },
            command::Action::Window(action) => match action {
                window::Action::Drag(id) => {
                    let _res = window.drag_window();
                }
                window::Action::Resize(id, size) => {
                    window.request_inner_size(winit::dpi::LogicalSize {
                        width: size.width,
                        height: size.height,
                    });
                }
                window::Action::Maximize(id, value) => {
                    window.set_maximized(value);
                }
                window::Action::Minimize(id, value) => {
                    window.set_minimized(value);
                }
                window::Action::Move(id, point) => {
                    window.set_outer_position(winit::dpi::PhysicalPosition {
                        x: point.x,
                        y: point.y,
                    });
                }
                window::Action::ChangeMode(id, mode) => {
                    window.set_visible(conversion::visible(mode));
                    window.set_fullscreen(conversion::fullscreen(window.primary_monitor(), mode));
                }
                window::Action::FetchMode(id, tag) => {
                    let mode = if window.is_visible().unwrap_or(true) {
                        conversion::mode(window.fullscreen())
                    } else {
                        iced_core::window::Mode::Hidden
                    };

                    state.queue_message(tag(mode));
                }
                window::Action::ToggleMaximize(id) => window.set_maximized(!window.is_maximized()),

                window::Action::ToggleDecorations(id) => {
                    window.set_decorations(!window.is_decorated())
                }
                window::Action::RequestUserAttention(id, user_attention) => {
                    window.request_user_attention(user_attention.map(conversion::user_attention))
                }
                window::Action::GainFocus(id) => window.focus_window(),
                window::Action::FetchId(id, tag) => {
                    state.queue_message(tag(window.id().into()));
                }
                window::Action::ChangeIcon(id, icon) => {
                    window.set_window_icon(conversion::icon(icon))
                }
                window::Action::FetchSize(id, callback) => {
                    let size = window.inner_size();
                    state.queue_message(callback(Size::new(size.width as f32, size.height as f32)))
                }
                window::Action::ChangeLevel(id, level) => {
                    window.set_window_level(conversion::window_level(level));
                }
                // too complicated
                window::Action::Screenshot(id, _) => todo!(),
                window::Action::Close(id) => todo!(),
                window::Action::Spawn(_, _) => todo!(),
                window::Action::FetchMaximized(_, _) => todo!(),
                window::Action::FetchMinimized(_, _) => todo!(),
                window::Action::ShowSystemMenu(_) => todo!(),
                window::Action::RunWithHandle(_, _) => todo!(),
            },
            command::Action::Widget(action) => {
                state.operate(renderer, Some(action).into_iter(), bounds, debug);
            }
            // too complicated
            command::Action::System(_) => todo!(),
            command::Action::LoadFont { .. } => todo!(),
            command::Action::Stream(_) => todo!(),
            command::Action::Custom(_) => todo!(),
        }
    }
}
