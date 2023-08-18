use iced_futures::{Executor, Runtime};
use iced_style::application::StyleSheet;
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
    <<P as Program>::Renderer as iced_core::Renderer>::Theme: StyleSheet,
{
    use iced_widget::runtime::{command, window};

    for action in command.actions() {
        match action {
            command::Action::Future(future) => {
                runtime.spawn(future);
            }
            command::Action::Clipboard(action) => match action {
                clipboard::Action::Read(tag) => {
                    state.queue_message(tag(clipboard.read()));
                }
                clipboard::Action::Write(contents) => {
                    clipboard.write(contents);
                }
            },
            command::Action::Window(action) => match action {
                window::Action::Drag => {
                    let _res = window.drag_window();
                }
                window::Action::Resize(size) => {
                    window.set_inner_size(winit::dpi::LogicalSize {
                        width: size.width,
                        height: size.height,
                    });
                }
                window::Action::Maximize(value) => {
                    window.set_maximized(value);
                }
                window::Action::Minimize(value) => {
                    window.set_minimized(value);
                }
                window::Action::Move { x, y } => {
                    window.set_outer_position(winit::dpi::LogicalPosition { x, y });
                }
                window::Action::ChangeMode(mode) => {
                    window.set_visible(conversion::visible(mode));
                    window.set_fullscreen(conversion::fullscreen(window.primary_monitor(), mode));
                }
                window::Action::FetchMode(tag) => {
                    let mode = if window.is_visible().unwrap_or(true) {
                        conversion::mode(window.fullscreen())
                    } else {
                        iced_core::window::Mode::Hidden
                    };

                    state.queue_message(tag(mode));
                }
                window::Action::ToggleMaximize => window.set_maximized(!window.is_maximized()),

                window::Action::ToggleDecorations => window.set_decorations(!window.is_decorated()),
                window::Action::RequestUserAttention(user_attention) => {
                    window.request_user_attention(user_attention.map(conversion::user_attention))
                }
                window::Action::GainFocus => window.focus_window(),
                window::Action::FetchId(tag) => {
                    state.queue_message(tag(window.id().into()));
                }
                window::Action::ChangeIcon(icon) => window.set_window_icon(conversion::icon(icon)),
                window::Action::FetchSize(callback) => {
                    let size = window.inner_size();
                    state.queue_message(callback(Size::new(size.width, size.height)))
                }
                window::Action::ChangeLevel(level) => {
                    window.set_window_level(conversion::window_level(level));
                }
                // too complicated
                window::Action::Screenshot(_) => todo!(),
                window::Action::Close => todo!(),
            },
            command::Action::Widget(action) => {
                state.operate(renderer, Some(action).into_iter(), bounds, debug);
            }
            // too complicated
            command::Action::System(_) => todo!(),
            command::Action::LoadFont { .. } => todo!(),
        }
    }
}
