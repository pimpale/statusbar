use iced_winit::alignment::{self, Alignment};
use iced_winit::widget::{button, column, container, row, scrollable, text, text_input};
use iced_winit::{theme, Command, Length};
use iced_winit::{Element, Program};

use iced_wgpu::{Color, Renderer};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

static INPUT_ID: Lazy<text_input::Id> = Lazy::new(text_input::Id::unique);
static ACTIVE_INPUT_ID: Lazy<text_input::Id> = Lazy::new(text_input::Id::unique);

#[derive(Debug)]
pub struct Todos {
    expanded: bool,
    state: State,
}

#[derive(Debug)]
pub enum State {
    Loading,
    Loaded(LoadedState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskCompletionKind {
    Success,
    Failure,
    Obsoleted,
}

#[derive(Debug)]
pub struct LoadedState {
    input_value: String,
    active_index: Option<usize>,
    live_tasks: VecDeque<String>,
    finished_tasks: Vec<(String, TaskCompletionKind)>,
}

impl Default for LoadedState {
    fn default() -> Self {
        LoadedState {
            input_value: String::new(),
            active_index: None,
            live_tasks: VecDeque::new(),
            finished_tasks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
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
}

impl Todos {
    pub fn new() -> Todos {
        Todos {
            expanded: false,
            // state: State::Loading,
            state: State::Loaded(LoadedState::default()),
        }
    }

    pub fn height(&self) -> u32 {
        match self.expanded {
            true => 250,
            false => 50,
        }
    }
}

impl Program for Todos {
    type Message = Message;
    type Renderer = Renderer;

    // the cringe runner doesn't actually run crap - check main for what actually does happen
    // "cross platform" when anyone needs to hook in platform-specifc stuff for each plaform is pain
    fn update(&mut self, message: Message) -> Command<Message> {
        match &mut self.state {
            State::Loading => Command::none(),
            State::Loaded(state) => {
                match message {
                    Message::CollapseDock => {
                        self.expanded = false;
                        state.input_value = String::new();
                        state.active_index = None;
                        Command::none()
                    }
                    Message::ExpandDock => {
                        self.expanded = true;
                        Command::none()
                    }
                    Message::EditInput(value) => {
                        state.input_value = value;
                        Command::none()
                    }
                    Message::PushInput => {
                        state
                            .live_tasks
                            .push_front(std::mem::take(&mut state.input_value));
                        state.active_index = state.active_index.map(|x| x + 1);
                        Command::none()
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
                        if let Some(i) = a {
                            text_input::focus(ACTIVE_INPUT_ID.clone())
                        } else {
                            Command::none()
                        }
                    }
                }
            }
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
            } => {
                let input = text_input("What needs to be done?", input_value, Message::EditInput)
                    .id(INPUT_ID.clone())
                    .on_submit(Message::PushInput);

                let tasks: Element<_, Renderer> = if live_tasks.len() > 0 {
                    column(
                        live_tasks
                            .iter()
                            .enumerate()
                            .map(|(i, task)| {
                                let header = text(format!("{}|", i + 1)).size(30);

                                match active_index {
                                    Some(idx) if i == *idx => row(vec![
                                        header.into(),
                                        button("Task Succeeded")
                                            .style(theme::Button::Positive)
                                            .on_press(Message::PopActive(
                                                TaskCompletionKind::Success,
                                            ))
                                            .into(),
                                        text_input("Edit Task", task, Message::EditActive)
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
                    .spacing(10)
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
}
