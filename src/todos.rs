use derivative::Derivative;
use futures_util::{FutureExt, Sink, SinkExt, Stream, TryFutureExt};
use iced_native::command::Action;
use iced_native::widget;
use iced_winit::alignment;
use iced_winit::widget::{button, column, container, row, scrollable, text};
use iced_winit::Element;
use iced_winit::{theme, Command, Length};

use iced_wgpu::Renderer;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use todoproxy_api::request::WebsocketInitMessage;
use todoproxy_api::{StateSnapshot, TaskStatus, WebsocketOp, WebsocketOpKind};
use tokio_tungstenite::tungstenite;

use crate::advanced_text_input;
use crate::utils;
use crate::wm_hints;

use crate::program_runner::ProgramWithSubscription;

// username and password text boxes
static USERNAME_INPUT_ID: Lazy<advanced_text_input::Id> =
    Lazy::new(advanced_text_input::Id::unique);
static PASSWORD_INPUT_ID: Lazy<advanced_text_input::Id> =
    Lazy::new(advanced_text_input::Id::unique);

// logged in boxes
static INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);
static ACTIVE_INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);

#[derive(Debug)]
pub struct Todos {
    wm_state: wm_hints::WmHintsState,
    focused: bool,
    expanded: bool,
    state: State,
}

#[derive(Debug)]
pub enum State {
    NotLoggedIn(NotLoggedInState),
    NotConnected(NotConnectedState),
    Connected(ConnectedState),
}

#[derive(Debug)]
pub struct NotLoggedInState {
    username: String,
    password: String,
    view_password: bool,
    error: Option<String>,
}

#[derive(Debug)]
pub struct NotConnectedState {
    api_key: String,
    error: Option<String>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectedState {
    api_key: String,
    #[derivative(Debug = "ignore")]
    websocket_recv: Box<
        dyn Stream<Item = Result<tungstenite::protocol::Message, tungstenite::error::Error>>
            + Unpin
            + Send,
    >,
    #[derivative(Debug = "ignore")]
    websocket_send: Box<
        dyn Sink<tungstenite::protocol::Message, Error = tungstenite::error::Error> + Unpin + Send,
    >,
    input_value: String,
    active_index: Option<usize>,
    snapshot: StateSnapshot,
}

impl Default for NotLoggedInState {
    fn default() -> Self {
        NotLoggedInState {
            username: String::new(),
            password: String::new(),
            view_password: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    EventOccurred(iced_native::Event),
    // Focus
    FocusDock,
    UnfocusDock,
    // change dock
    ExpandDock,
    CollapseDock,

    // not logged in page
    EditUsername(String),
    SubmitUsername,
    EditPassword(String),
    SubmitPassword,
    TogglePasswordView,

    // not connected page
    RetryConnect,
    // connected page
    EditInput(String),
    SubmitInput,
    EditActive(String),
    SetActive(Option<usize>),
    Op(Op),
    // Websocket Interactions
    WebsocketSendComplete(Result<(), String>),
    WebsocketRecvComplete(Result<WebsocketOp, String>),
    // debugging
    Yeet,
}

#[derive(Debug, Clone)]
pub enum Op {
    NewLive(String, usize),
    RestoreFinished,
    Pop(usize, TaskStatus),
    Edit(usize, String),
    Move(usize, usize),
}

impl Todos {
    pub fn new(wm_state: wm_hints::WmHintsState) -> Todos {
        Todos {
            wm_state,
            expanded: false,
            focused: false,
            // state: State::Loading,
            state: State::NotLoggedIn(NotLoggedInState::default()),
        }
    }

    // creates a command from an operation
    // (operation must be valid)
    fn wsop<S>(state: &StateSnapshot, sink: &mut S, op: Op) -> Command<Message>
    where
        S: Sink<tungstenite::protocol::Message, Error = tungstenite::error::Error> + Unpin + Send,
    {
        // create op
        let wsop = WebsocketOp {
            alleged_time: utils::current_time_millis(),
            kind: match op {
                Op::NewLive(value, position) => WebsocketOpKind::InsLiveTask {
                    value,
                    id: utils::random_string(),
                    position,
                },
                Op::RestoreFinished => WebsocketOpKind::RestoreFinishedTask {
                    id: state.finished.first().unwrap().id.clone(),
                },
                Op::Pop(position, status) => WebsocketOpKind::FinishLiveTask {
                    id: state.live[position].id.clone(),
                    status,
                },
                Op::Edit(position, value) => WebsocketOpKind::EditLiveTask {
                    id: state.live[position].id.clone(),
                    value,
                },
                Op::Move(del, ins) => WebsocketOpKind::MvLiveTask {
                    id_del: state.live[del].id.clone(),
                    id_ins: state.live[ins].id.clone(),
                },
            },
        };

        // send op
        let future = sink
            .feed(tungstenite::protocol::Message::Text(
                serde_json::to_string(&wsop).unwrap(),
            ))
            .map_err(|e| e.to_string())
            .map(Message::WebsocketSendComplete);

        Command::single(Action::Future(Box::pin(future)))
    }
}

impl ProgramWithSubscription for Todos {
    type Message = Message;
    type Renderer = Renderer;

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::Yeet => {
                println!("yeet");
                Command::none()
            }
            Message::EventOccurred(event) => match event {
                // grab keyboard focus on cursor enter
                iced_native::Event::Mouse(iced_native::mouse::Event::CursorEntered) => {
                    Command::single(Action::Future(Box::pin(async { Message::FocusDock })))
                }
                // release keyboard focus on cursor exit
                iced_native::Event::Mouse(iced_native::mouse::Event::CursorLeft) => {
                    Command::single(Action::Future(Box::pin(async { Message::UnfocusDock })))
                }
                _ => Command::none(),
            },
            Message::FocusDock => {
                self.focused = true;
                if self.expanded {
                    wm_hints::grab_keyboard(&self.wm_state).unwrap();
                }
                Command::none()
            }
            Message::UnfocusDock => {
                if self.expanded {
                    wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                }
                self.focused = false;
                Command::none()
            }
            Message::ExpandDock => {
                self.expanded = true;

                // grab keyboard focus
                if !self.focused {
                    wm_hints::grab_keyboard(&self.wm_state).unwrap();
                    self.focused = true;
                }

                let command = match self.state {
                    State::Connected(state) => advanced_text_input::focus(INPUT_ID.clone()),
                    _ => Command::none(),
                };

                Command::batch([iced_winit::window::resize(1, 250), command])
            }
            Message::CollapseDock => {
                self.expanded = false;
                if self.focused {
                    wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                }
                match self.state {
                    State::Connected(state) => {
                        state.input_value = String::new();
                        state.active_index = None;
                    }
                    _ => {}
                }

                iced_winit::window::resize(1, 50)
            }
            Message::EditInput(value) => {
                match self.state {
                    State::Connected(state) => {
                        state.input_value = value;
                    }
                    _ => {}
                }
                Command::none()
            }
            Message::SubmitInput => match self.state {
                State::Connected(state) => {
                    let val = std::mem::take(&mut state.input_value);
                    match val.split_once(" ").map(|x| x.0).unwrap_or(val.as_str()) {
                        "q" => Command::single(Action::Future(Box::pin(async {
                            Message::CollapseDock
                        }))),
                        "x" => panic!(),
                        "ps" => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::Pop(0, TaskStatus::Succeeded),
                        ),
                        "pf" => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::Pop(0, TaskStatus::Failed),
                        ),
                        "po" => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::Pop(0, TaskStatus::Obsoleted),
                        ),
                        "r" => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::RestoreFinished,
                        ),
                        "mv" => {
                            let op = if let Ok((i, j)) =
                                sscanf::scanf!(val, "mv {} {}", usize, usize)
                            {
                                if i < state.snapshot.live.len() && j < state.snapshot.live.len() {
                                    Some(Op::Move(i, j))
                                } else {
                                    None
                                }
                            } else if let Ok(i) = sscanf::scanf!(val, "mv {}", usize) {
                                if i < state.snapshot.live.len() {
                                    Some(Op::Move(i, 0))
                                } else {
                                    None
                                }
                            } else if val == "mv" {
                                if state.snapshot.live.len() >= 2 {
                                    Some(Op::Move(0, 1))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            match op {
                                Some(op) => {
                                    Self::wsop(&state.snapshot, &mut state.websocket_send, op)
                                }
                                None => Command::none(),
                            }
                        }
                        _ => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::NewLive(val, 0),
                        ),
                    }
                }
                _ => Command::none(),
            },
            Message::EditActive(value) => {
                match self.state {
                    State::Connected(state) => {
                        state.snapshot.live[state.active_index.unwrap()].value = value;
                    }
                    _ => {}
                }
                Command::none()
            }
            Message::SetActive(a) => match self.state {
                State::Connected(state) => {
                    let command = match state.active_index {
                        Some(position) => Self::wsop(
                            &state.snapshot,
                            &mut state.websocket_send,
                            Op::Edit(position, state.snapshot.live[position].value.clone()),
                        ),
                        None => Command::none(),
                    };
                    state.active_index = a;
                    Command::batch([
                        command,
                        match a {
                            Some(x) => advanced_text_input::focus(ACTIVE_INPUT_ID.clone()),
                            None => Command::none(),
                        },
                    ])
                }
                _ => Command::none(),
            },
            Message::Op(op) => match self.state {
                State::Connected(state) => {
                    Self::wsop(&state.snapshot, &mut state.websocket_send, op)
                }
                _ => Command::none(),
            },
            Message::EditUsername(val) => match self.state {
                State::NotLoggedIn(state) => {
                    state.username = val;
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::SubmitUsername => match self.state {
                State::NotLoggedIn(state) => Command::single(Action::Widget(widget::Action::new(
                    widget::operation::focusable::focus_next(),
                ))),
                _ => Command::none(),
            },
            Message::EditPassword(val) => match self.state {
                State::NotLoggedIn(state) => {
                    state.password = val;
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::SubmitPassword => match self.state {
                State::NotLoggedIn(state) => Command::single(Action::Widget(widget::Action::new(
                    widget::operation::focusable::focus_next(),
                ))),
                _ => Command::none(),
            },
            Message::TogglePasswordView => match self.state {
                State::NotLoggedIn(state) => {
                    state.view_password = !state.view_password;
                    Command::none()
                }
                _ => Command::none()
                },
            Message::RetryConnect => todo!(),
            Message::WebsocketSendComplete(result) => todo!(),
            Message::WebsocketRecvComplete(_) => todo!(),
        }
    }

    fn view(&self) -> Element<Message, Renderer> {
        match self {
            Self {
                state: State::Loading,
                ..
            } => container(text("Loading...").size(30))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x()
                .center_y()
                .into(),
            Self {
                state:
                    State::Loaded(LoadedState {
                        input_value,
                        live_tasks,
                        active_index,
                        ..
                    }),
                expanded: true,
                ..
            } => {
                let input = advanced_text_input::AdvancedTextInput::new(
                    "What needs to be done?",
                    input_value,
                    Message::EditInput,
                )
                .id(INPUT_ID.clone())
                .on_focus(Message::SetActive(None))
                .on_submit(Message::PushInput);

                let tasks: Element<_, Renderer> = if live_tasks.len() > 0 {
                    column(
                        live_tasks
                            .iter()
                            .enumerate()
                            .map(|(i, task)| {
                                let header = text(format!("{}|", i)).size(25);

                                match active_index {
                                    Some(idx) if i == *idx => row(vec![
                                        header.into(),
                                        button("Task Succeeded")
                                            .style(theme::Button::Positive)
                                            .on_press(Message::Op(Op::Pop(
                                                i,
                                                TaskStatus::Succeeded,
                                            )))
                                            .into(),
                                        advanced_text_input::AdvancedTextInput::new(
                                            "Edit Task",
                                            task,
                                            Message::EditActive,
                                        )
                                        .id(ACTIVE_INPUT_ID.clone())
                                        .on_submit(Message::SetActive(None))
                                        .into(),
                                        button("Task Failed")
                                            .style(theme::Button::Destructive)
                                            .on_press(Message::Op(Op::Pop(i, TaskStatus::Failed)))
                                            .into(),
                                        button("Task Obsoleted")
                                            .style(theme::Button::Secondary)
                                            .on_press(Message::Op(Op::Pop(
                                                i,
                                                TaskStatus::Obsoleted,
                                            )))
                                            .into(),
                                    ])
                                    .spacing(10)
                                    .into(),
                                    _ => row(vec![
                                        header.into(),
                                        button(text(&task))
                                            .on_press(Message::SetActive(Some(i)))
                                            .style(theme::Button::Text)
                                            .width(Length::Fill)
                                            .into(),
                                    ])
                                    .spacing(10)
                                    .into(),
                                }
                            })
                            .collect(),
                    )
                    // pad right to avoid clipping scrollable
                    .padding([0, 15, 0, 0])
                    .into()
                } else {
                    text("You have not created a task yet...").size(25).into()
                };

                row(vec![
                    button("Collapse").on_press(Message::CollapseDock).into(),
                    column(vec![input.into(), scrollable(tasks).into()])
                        .spacing(10)
                        .width(Length::Shrink)
                        .into(),
                ])
                .spacing(10)
                .padding(10)
                .into()
            }
            Self {
                state: State::Loaded(LoadedState { live_tasks, .. }),
                expanded: false,
                ..
            } => match live_tasks.front() {
                None => container(button("Click to Add Task").on_press(Message::ExpandDock))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x()
                    .center_y()
                    .padding(10)
                    .into(),
                Some(task) => container(
                    row(vec![
                        button("Task Succeeded")
                            .height(Length::Fill)
                            .style(theme::Button::Positive)
                            .on_press(Message::PopTopmost(TaskCompletionKind::Success))
                            .into(),
                        button(text(task).horizontal_alignment(alignment::Horizontal::Center))
                            .height(Length::Fill)
                            .width(Length::Fill)
                            .style(theme::Button::Text)
                            .on_press(Message::ExpandDock)
                            .into(),
                        button("Task Failed")
                            .height(Length::Fill)
                            .style(theme::Button::Destructive)
                            .on_press(Message::PopTopmost(TaskCompletionKind::Failure))
                            .into(),
                        button("Task Obsoleted")
                            .height(Length::Fill)
                            .style(theme::Button::Secondary)
                            .on_press(Message::PopTopmost(TaskCompletionKind::Obsoleted))
                            .into(),
                    ])
                    .height(Length::Fill)
                    .spacing(10),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x()
                .center_y()
                .padding(10)
                .into(),
            },
        }
    }

    fn handle_uncaptured_events(&self, events: Vec<iced_native::Event>) -> Vec<Self::Message> {
        events.into_iter().map(Message::EventOccurred).collect()
    }
}
