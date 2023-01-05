use futures_util::{Stream, Sink};
use iced_winit::alignment;
use iced_winit::widget::{button, column, container, row, scrollable, text};
use iced_winit::Element;
use iced_winit::{theme, Command, Length};

use iced_wgpu::Renderer;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{WebSocketStream, tungstenite};
use tokio_tungstenite::tungstenite::stream::MaybeTlsStream;
use std::collections::VecDeque;

use crate::advanced_text_input;
use crate::wm_hints;

use crate::program_runner::ProgramWithSubscription;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskCompletionKind {
    Success,
    Failure,
    Obsoleted,
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

#[derive(Debug)]
pub struct ConnectedState {
    api_key: String,
    websocket_recv: Box<dyn Stream<Item = Result<tungstenite::protocol::Message, tungstenite::error::Error>> + Debug>,
    websocket_send: Box<dyn Sink<tungstenite::protocol::Message, Error = tungstenite::error::Error>>,
    input_value: String,
    active_index: Option<usize>,
    live_tasks: VecDeque<String>,
    finished_tasks: Vec<(String, TaskCompletionKind)>,
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
    // change dock
    ExpandDock,
    CollapseDock,
    // operations performed on the input_value
    EditInput(String),
    PushInput,
    QueueInput,
    // operations performed on the currently active index
    EditActive(String),
    QueueActive,
    PopActive(TaskCompletionKind),
    // operations on the topmost value
    PopTopmost(TaskCompletionKind),
    // set active
    SetActive(Option<usize>),
    Yeet,
}

impl Todos {
    pub fn new(wm_state: wm_hints::WmHintsState) -> Todos {
        Todos {
            wm_state,
            expanded: false,
            focused: false,
            // state: State::Loading,
            state: State::Loaded(LoadedState::default()),
        }
    }
}

impl ProgramWithSubscription for Todos {
    type Message = Message;
    type Renderer = Renderer;

    // the cringe runner doesn't actually run crap - check main for what actually does happen
    // "cross platform" when anyone needs to hook in platform-specifc stuff for each plaform is pain
    fn update(&mut self, message: Message) -> Command<Message> {
        match &mut self.state {
            State::Loading => Command::none(),
            State::Loaded(state) => match message {
                Message::Yeet => {
                    println!("yeet");
                    Command::none()
                }
                Message::EventOccurred(event) => {
                    match event {
                        // grab keyboard focus on cursor enter
                        iced_native::Event::Mouse(iced_native::mouse::Event::CursorEntered) => {
                            if self.expanded && !self.focused {
                                wm_hints::grab_keyboard(&self.wm_state).unwrap();
                                self.focused = true;
                            }
                        }
                        // release keyboard focus on cursor exit
                        iced_native::Event::Mouse(iced_native::mouse::Event::CursorLeft) => {
                            if self.expanded && self.focused {
                                wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                                self.focused = false;
                            }
                        }
                        _ => {}
                    }
                    Command::none()
                }
                Message::CollapseDock => {
                    self.expanded = false;
                    state.input_value = String::new();
                    state.active_index = None;

                    // release keyboard focus
                    if self.focused {
                        wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                        self.focused = false;
                    }

                    iced_winit::window::resize(1, 50)
                }
                Message::ExpandDock => {
                    self.expanded = true;

                    // grab keyboard focus
                    if !self.focused {
                        wm_hints::grab_keyboard(&self.wm_state).unwrap();
                        self.focused = true;
                    }

                    Command::batch([
                        iced_winit::window::resize(1, 250),
                        advanced_text_input::focus(INPUT_ID.clone()),
                    ])
                }
                Message::EditInput(value) => {
                    state.input_value = value;
                    Command::none()
                }
                Message::PushInput => {
                    let val = std::mem::take(&mut state.input_value);
                    match val.split_once(" ").map(|x| x.0).unwrap_or(val.as_str()) {
                        "q" => {
                            self.expanded = false;
                            state.input_value = String::new();
                            state.active_index = None;

                            // release keyboard focus
                            if self.focused {
                                wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                                self.focused = false;
                            }

                            iced_winit::window::resize(1, 50)
                        }
                        "x" => panic!(),
                        "po" => {
                            if let Some(task) = state.live_tasks.pop_front() {
                                state
                                    .finished_tasks
                                    .push((task, TaskCompletionKind::Obsoleted));
                            }
                            Command::none()
                        }
                        "ps" => {
                            if let Some(task) = state.live_tasks.pop_front() {
                                state
                                    .finished_tasks
                                    .push((task, TaskCompletionKind::Success));
                            }
                            Command::none()
                        }
                        "pf" => {
                            if let Some(task) = state.live_tasks.pop_front() {
                                state
                                    .finished_tasks
                                    .push((task, TaskCompletionKind::Failure));
                            }
                            Command::none()
                        }
                        "r" => {
                            if let Some((task, _)) = state.finished_tasks.pop() {
                                state.live_tasks.push_front(task);
                            }
                            Command::none()
                        }
                        "swp" => {
                            if let Ok((i, j)) = sscanf::scanf!(val, "swp {} {}", usize, usize) {
                                if i < state.live_tasks.len() && j < state.live_tasks.len() {
                                    state.live_tasks.swap(i, j);
                                }
                            } else if let Ok(i) = sscanf::scanf!(val, "swp {}", usize) {
                                if i < state.live_tasks.len() {
                                    state.live_tasks.swap(0, i);
                                }
                            } else if val == "swp" {
                                if state.live_tasks.len() >= 2 {
                                    state.live_tasks.swap(0, 1);
                                }
                            }
                            Command::none()
                        }
                        _ => {
                            state.live_tasks.push_front(val);
                            state.active_index = state.active_index.map(|x| x + 1);
                            Command::none()
                        }
                    }
                }
                Message::QueueInput => {
                    state
                        .live_tasks
                        .push_back(std::mem::take(&mut state.input_value));
                    Command::none()
                }
                Message::EditActive(value) => {
                    state.live_tasks[state.active_index.unwrap()] = value;
                    Command::none()
                }
                Message::QueueActive => {
                    let value = state
                        .live_tasks
                        .remove(state.active_index.unwrap())
                        .unwrap();
                    state.live_tasks.push_back(value);
                    state.active_index = Some(state.live_tasks.len() - 1);
                    Command::none()
                }
                Message::PopActive(kind) => {
                    let value = state
                        .live_tasks
                        .remove(state.active_index.unwrap())
                        .unwrap();
                    state.finished_tasks.push((value, kind));
                    state.active_index = None;
                    Command::none()
                }
                Message::PopTopmost(kind) => {
                    if let Some(task) = state.live_tasks.pop_front() {
                        state.finished_tasks.push((task, kind));
                    }
                    Command::none()
                }
                Message::SetActive(a) => {
                    state.active_index = a;
                    if a.is_some() {
                        advanced_text_input::focus(ACTIVE_INPUT_ID.clone())
                    } else {
                        Command::none()
                    }
                }
            },
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
                                            .on_press(Message::PopActive(
                                                TaskCompletionKind::Success,
                                            ))
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
                                            .on_press(Message::PopActive(
                                                TaskCompletionKind::Failure,
                                            ))
                                            .into(),
                                        button("Task Obsoleted")
                                            .style(theme::Button::Secondary)
                                            .on_press(Message::PopActive(
                                                TaskCompletionKind::Obsoleted,
                                            ))
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
