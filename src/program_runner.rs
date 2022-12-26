use iced_winit::widget::operation;
// sourced from: https://github.com/iced-rs/iced/blob/master/native/src/program/state.rs
use iced_winit::event::{self, Event};
use iced_winit::mouse;
use iced_winit::renderer;
use iced_winit::user_interface::{self, UserInterface};
use iced_winit::{clipboard, conversion, winit, Executor, Proxy, Runtime};
use iced_winit::{Clipboard, Command, Debug, Point, Program, Size};

/// The execution state of a [`Program`]. It leverages caching, event
/// processing, and rendering primitive storage.
#[allow(missing_debug_implementations)]
pub struct State<P>
where
    P: Program + 'static,
{
    program: P,
    cache: user_interface::Cache,
    queued_events: Vec<Event>,
    queued_messages: Vec<P::Message>,
    mouse_interaction: mouse::Interaction,
}

impl<P> State<P>
where
    P: Program + 'static,
    <<P as Program>::Renderer as iced_winit::Renderer>::Theme: iced_winit::application::StyleSheet,
{
    /// Creates a new [`State`] with the provided [`Program`], initializing its
    /// primitive with the given logical bounds and renderer.
    pub fn new(
        mut program: P,
        bounds: Size,
        renderer: &mut P::Renderer,
        debug: &mut Debug,
    ) -> Self {
        let user_interface = build_user_interface(
            &mut program,
            user_interface::Cache::default(),
            renderer,
            bounds,
            debug,
        );

        let cache = user_interface.into_cache();

        State {
            program,
            cache,
            queued_events: Vec::new(),
            queued_messages: Vec::new(),
            mouse_interaction: mouse::Interaction::Idle,
        }
    }

    /// Returns a reference to the [`Program`] of the [`State`].
    pub fn program(&self) -> &P {
        &self.program
    }

    /// Queues an event in the [`State`] for processing during an [`update`].
    ///
    /// [`update`]: Self::update
    pub fn queue_event(&mut self, event: Event) {
        self.queued_events.push(event);
    }

    /// Queues a message in the [`State`] for processing during an [`update`].
    ///
    /// [`update`]: Self::update
    pub fn queue_message(&mut self, message: P::Message) {
        self.queued_messages.push(message);
    }

    /// Returns whether the event queue of the [`State`] is empty or not.
    pub fn is_queue_empty(&self) -> bool {
        self.queued_events.is_empty() && self.queued_messages.is_empty()
    }

    /// Returns the current [`mouse::Interaction`] of the [`State`].
    pub fn mouse_interaction(&self) -> mouse::Interaction {
        self.mouse_interaction
    }

    /// Processes all the queued events and messages, rebuilding and redrawing
    /// the widgets of the linked [`Program`] if necessary.
    ///
    /// Returns a list containing the instances of [`Event`] that were not
    /// captured by any widget, and the [`Command`] obtained from [`Program`]
    /// after updating it, only if an update was necessary.
    pub fn update(
        &mut self,
        bounds: Size,
        cursor_position: Point,
        renderer: &mut P::Renderer,
        theme: &<P::Renderer as iced_winit::Renderer>::Theme,
        style: &renderer::Style,
        clipboard: &mut Clipboard,
        debug: &mut Debug,
    ) -> (Vec<Event>, Option<Command<P::Message>>) {
        let mut user_interface = build_user_interface(
            &mut self.program,
            std::mem::take(&mut self.cache),
            renderer,
            bounds,
            debug,
        );

        debug.event_processing_started();
        let mut messages = Vec::new();

        let (_, event_statuses) = user_interface.update(
            &self.queued_events,
            cursor_position,
            renderer,
            clipboard,
            &mut messages,
        );

        let uncaptured_events = self
            .queued_events
            .iter()
            .zip(event_statuses)
            .filter_map(|(event, status)| matches!(status, event::Status::Ignored).then_some(event))
            .cloned()
            .collect();

        self.queued_events.clear();
        messages.append(&mut self.queued_messages);
        debug.event_processing_finished();

        let command = if messages.is_empty() {
            debug.draw_started();
            self.mouse_interaction = user_interface.draw(renderer, theme, style, cursor_position);
            debug.draw_finished();

            self.cache = user_interface.into_cache();

            None
        } else {
            // When there are messages, we are forced to rebuild twice
            // for now :^)
            let temp_cache = user_interface.into_cache();

            let commands = Command::batch(messages.into_iter().map(|message| {
                debug.log_message(&message);

                debug.update_started();
                let command = self.program.update(message);
                debug.update_finished();

                command
            }));

            let mut user_interface =
                build_user_interface(&mut self.program, temp_cache, renderer, bounds, debug);

            debug.draw_started();
            self.mouse_interaction = user_interface.draw(renderer, theme, style, cursor_position);
            debug.draw_finished();

            self.cache = user_interface.into_cache();

            Some(commands)
        };

        (uncaptured_events, command)
    }

    /// Runs the actions of a [`Command`].
    pub fn run_command<E>(
        &mut self,
        command: Command<P::Message>,
        bounds: Size,
        cursor_position: Point,
        renderer: &mut P::Renderer,
        theme: &<P::Renderer as iced_winit::Renderer>::Theme,
        style: &renderer::Style,
        runtime: &mut Runtime<E, Proxy<P::Message>, P::Message>,
        clipboard: &mut Clipboard,
        proxy: &mut winit::event_loop::EventLoopProxy<P::Message>,
        debug: &mut Debug,
        window: &winit::window::Window,
    ) where
        E: Executor,
    {
        run_command(
            &mut self.program,
            bounds,
            &mut self.cache,
            renderer,
            command,
            runtime,
            clipboard,
            proxy,
            debug,
            window,
        );

        let mut user_interface = build_user_interface(
            &mut self.program,
            std::mem::take(&mut self.cache),
            renderer,
            bounds,
            debug,
        );

        debug.draw_started();
        self.mouse_interaction = user_interface.draw(renderer, theme, style, cursor_position);
        debug.draw_finished();

        self.cache = user_interface.into_cache();
    }
}

fn build_user_interface<'a, P: Program>(
    program: &'a mut P,
    cache: user_interface::Cache,
    renderer: &mut P::Renderer,
    size: Size,
    debug: &mut Debug,
) -> UserInterface<'a, P::Message, P::Renderer>
where
    <<P as Program>::Renderer as iced_winit::Renderer>::Theme: iced_winit::application::StyleSheet,
{
    debug.view_started();
    let view = program.view();
    debug.view_finished();

    debug.layout_started();
    let user_interface = UserInterface::build(view, size, cache, renderer);
    debug.layout_finished();

    user_interface
}

/// Runs the actions of a [`Command`].
fn run_command<P, E>(
    program: &mut P,
    bounds: Size,
    cache: &mut user_interface::Cache,
    renderer: &mut P::Renderer,
    command: Command<P::Message>,
    runtime: &mut Runtime<E, Proxy<P::Message>, P::Message>,
    clipboard: &mut Clipboard,
    proxy: &mut winit::event_loop::EventLoopProxy<P::Message>,
    debug: &mut Debug,
    window: &winit::window::Window,
) where
    P: Program,
    E: Executor,
    <<P as Program>::Renderer as iced_winit::Renderer>::Theme: iced_winit::application::StyleSheet,
{
    use iced_native::command;
    use iced_native::window;

    for action in command.actions() {
        match action {
            command::Action::Future(future) => {
                runtime.spawn(future);
            }
            command::Action::Clipboard(action) => match action {
                clipboard::Action::Read(tag) => {
                    let message = tag(clipboard.read());

                    proxy
                        .send_event(message)
                        .expect("Send message to event loop");
                }
                clipboard::Action::Write(contents) => {
                    clipboard.write(contents);
                }
            },
            command::Action::Window(action) => match action {
                window::Action::Drag => {
                    let _res = window.drag_window();
                }
                window::Action::Resize { width, height } => {
                    window.set_inner_size(winit::dpi::LogicalSize { width, height });
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
                window::Action::SetMode(mode) => {
                    window.set_visible(conversion::visible(mode));
                    window.set_fullscreen(conversion::fullscreen(window.primary_monitor(), mode));
                }
                window::Action::ToggleMaximize => window.set_maximized(!window.is_maximized()),
                window::Action::FetchMode(tag) => {
                    let mode = if window.is_visible().unwrap_or(true) {
                        conversion::mode(window.fullscreen())
                    } else {
                        iced_winit::window::Mode::Hidden
                    };

                    proxy
                        .send_event(tag(mode))
                        .expect("Send message to event loop");
                }
            },
            command::Action::Widget(action) => {
                let mut current_cache = std::mem::take(cache);
                let mut current_operation = Some(action.into_operation());

                let mut user_interface =
                    build_user_interface(program, current_cache, renderer, bounds, debug);

                while let Some(mut operation) = current_operation.take() {
                    user_interface.operate(renderer, operation.as_mut());

                    match operation.finish() {
                        operation::Outcome::None => {}
                        operation::Outcome::Some(message) => {
                            proxy
                                .send_event(message)
                                .expect("Send message to event loop");
                        }
                        operation::Outcome::Chain(next) => {
                            current_operation = Some(next);
                        }
                    }
                }

                current_cache = user_interface.into_cache();
                *cache = current_cache;
            }
            command::Action::System(_) => todo!(),
        }
    }
}
