use iced_winit::alignment::{self, Alignment};
use iced_winit::widget::{
    button, checkbox, column, container, row, scrollable, text, text_input, Text,
};
use iced_winit::{theme, Command, Length};
use iced_winit::{Element, Program};

use iced_wgpu::{Color, Renderer};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

static INPUT_ID: Lazy<text_input::Id> = Lazy::new(text_input::Id::unique);

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

#[derive(Debug)]
pub struct LoadedState {
    input_value: String,
    tasks: VecDeque<Task>,
    dirty: bool,
    saving: bool,
}

impl Default for LoadedState {
    fn default() -> Self {
        LoadedState {
            input_value: String::new(),
            tasks: VecDeque::new(),
            dirty: false,
            saving: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<SavedState, LoadError>),
    Saved(Result<(), SaveError>),
    InputChanged(String),
    CreateTask,
    Expand,
    Collapse,
    TaskMessage(usize, TaskMessage),
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
            true => 200,
            false => 50,
        }
    }
}

impl Program for Todos {
    type Message = Message;
    type Renderer = Renderer;

    fn update(&mut self, message: Message) -> Command<Message> {
        match &mut self.state {
            State::Loading => {
                match message {
                    Message::Loaded(Ok(state)) => {
                        self.state = State::Loaded(LoadedState {
                            input_value: state.input_value,
                            tasks: state.tasks.into(),
                            dirty: false,
                            saving: false,
                        });
                    }
                    Message::Loaded(Err(_)) => {
                        self.state = State::Loaded(LoadedState::default());
                    }
                    _ => {}
                }

                text_input::focus(INPUT_ID.clone())
            }
            State::Loaded(state) => {
                let mut saved = false;

                let command = match message {
                    Message::Collapse => {
                        self.expanded = false;
                        Command::none()
                    }
                    Message::Expand => {
                        self.expanded = true;
                        Command::none()
                    }
                    Message::InputChanged(value) => {
                        state.input_value = value;

                        Command::none()
                    }
                    Message::CreateTask => {
                        if !state.input_value.is_empty() {
                            state.tasks.push_front(Task::new(state.input_value.clone()));
                            state.input_value.clear();
                        }

                        Command::none()
                    }
                    Message::TaskMessage(i, TaskMessage::Delete) => {
                        state.tasks.remove(i);

                        Command::none()
                    }
                    Message::TaskMessage(i, task_message) => {
                        if let Some(task) = state.tasks.get_mut(i) {
                            let should_focus = matches!(task_message, TaskMessage::Edit);

                            task.update(task_message);

                            if should_focus {
                                let id = Task::text_input_id(i);
                                Command::batch(vec![
                                    text_input::focus(id.clone()),
                                    text_input::select_all(id),
                                ])
                            } else {
                                Command::none()
                            }
                        } else {
                            Command::none()
                        }
                    }
                    Message::Saved(_) => {
                        state.saving = false;
                        saved = true;

                        Command::none()
                    }
                    _ => Command::none(),
                };

                if !saved {
                    state.dirty = true;
                }

                let save = if state.dirty && !state.saving {
                    state.dirty = false;
                    state.saving = true;

                    Command::perform(
                        SavedState {
                            input_value: state.input_value.clone(),
                            tasks: state.tasks.clone().into(),
                        }
                        .save(),
                        Message::Saved,
                    )
                } else {
                    Command::none()
                };

                Command::batch(vec![command, save])
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
                        input_value, tasks, ..
                    }),
                expanded: true,
            } => {
                let input = text_input(
                    "What needs to be done?",
                    &input_value,
                    Message::InputChanged,
                )
                .id(INPUT_ID.clone())
                .on_submit(Message::CreateTask);

                let tasks: Element<_, Renderer> = if tasks.len() > 0 {
                    column(
                        tasks
                            .iter()
                            .enumerate()
                            .map(|(i, task)| {
                                task.view(i)
                                    .map(move |message| Message::TaskMessage(i, message))
                            })
                            .collect(),
                    )
                    .spacing(10)
                    .into()
                } else {
                    text("You have not created a task yet...").size(25).into()
                };

                row(vec![
                    button("Collapse").on_press(Message::Collapse).into(),
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
                state: State::Loaded(LoadedState { tasks, .. }),
                expanded: false,
            } => match tasks.front() {
                None => container(button("Click to Add Task").on_press(Message::Expand))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Left)
                    .align_y(alignment::Vertical::Top)
                    .padding(10)
                    .into(),
                Some(task) => row(vec![
                    button("Expand").on_press(Message::Expand).into(),
                    button(text(&task.description))
                        .height(Length::Fill)
                        .style(
                        .into(),
                ])
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(10)
                .padding(10)
                .into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    description: String,
    completed: bool,

    #[serde(skip)]
    state: TaskState,
}

#[derive(Debug, Clone)]
pub enum TaskState {
    Idle,
    Editing,
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone)]
pub enum TaskMessage {
    Completed(bool),
    Edit,
    DescriptionEdited(String),
    FinishEdition,
    Delete,
}

impl Task {
    fn text_input_id(i: usize) -> text_input::Id {
        text_input::Id::new(format!("task-{}", i))
    }

    fn new(description: String) -> Self {
        Task {
            description,
            completed: false,
            state: TaskState::Idle,
        }
    }

    fn update(&mut self, message: TaskMessage) {
        match message {
            TaskMessage::Completed(completed) => {
                self.completed = completed;
            }
            TaskMessage::Edit => {
                self.state = TaskState::Editing;
            }
            TaskMessage::DescriptionEdited(new_description) => {
                self.description = new_description;
            }
            TaskMessage::FinishEdition => {
                if !self.description.is_empty() {
                    self.state = TaskState::Idle;
                }
            }
            TaskMessage::Delete => {}
        }
    }

    fn view(&self, i: usize) -> Element<TaskMessage, Renderer> {
        match &self.state {
            TaskState::Idle => {
                let checkbox = checkbox(&self.description, self.completed, TaskMessage::Completed)
                    .width(Length::Fill);

                row(vec![
                    checkbox.into(),
                    button(edit_icon())
                        .on_press(TaskMessage::Edit)
                        .padding(10)
                        .style(theme::Button::Text)
                        .into(),
                ])
                .spacing(20)
                .align_items(Alignment::Center)
                .into()
            }
            TaskState::Editing => {
                let text_input = text_input(
                    "Describe your task...",
                    &self.description,
                    TaskMessage::DescriptionEdited,
                )
                .id(Self::text_input_id(i))
                .on_submit(TaskMessage::FinishEdition)
                .padding(10);

                row(vec![
                    text_input.into(),
                    button("Delete")
                        .on_press(TaskMessage::Delete)
                        .padding(10)
                        .style(theme::Button::Destructive)
                        .into(),
                ])
                .spacing(20)
                .align_items(Alignment::Center)
                .into()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Filter {
    Active,
    Completed,
}

fn icon(unicode: char) -> Text<'static, Renderer> {
    text(unicode.to_string())
        //.font(ICONS)
        .width(Length::Units(20))
        .horizontal_alignment(alignment::Horizontal::Center)
        .size(20)
}


// Persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedState {
    input_value: String,
    tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub enum LoadError {
    File,
    Format,
}

#[derive(Debug, Clone)]
pub enum SaveError {
    File,
    Write,
    Format,
}

impl SavedState {
    fn path() -> std::path::PathBuf {
        let mut path = if let Some(project_dirs) =
            directories_next::ProjectDirs::from("rs", "Iced", "Todos")
        {
            project_dirs.data_dir().into()
        } else {
            std::env::current_dir().unwrap_or_default()
        };

        path.push("todos.json");

        path
    }

    async fn load() -> Result<SavedState, LoadError> {
        use async_std::prelude::*;

        let mut contents = String::new();

        let mut file = async_std::fs::File::open(Self::path())
            .await
            .map_err(|_| LoadError::File)?;

        file.read_to_string(&mut contents)
            .await
            .map_err(|_| LoadError::File)?;

        serde_json::from_str(&contents).map_err(|_| LoadError::Format)
    }

    async fn save(self) -> Result<(), SaveError> {
        use async_std::prelude::*;

        let json = serde_json::to_string_pretty(&self).map_err(|_| SaveError::Format)?;

        let path = Self::path();

        if let Some(dir) = path.parent() {
            async_std::fs::create_dir_all(dir)
                .await
                .map_err(|_| SaveError::File)?;
        }

        {
            let mut file = async_std::fs::File::create(path)
                .await
                .map_err(|_| SaveError::File)?;

            file.write_all(json.as_bytes())
                .await
                .map_err(|_| SaveError::Write)?;
        }

        // This is a simple way to save at most once every couple seconds
        async_std::task::sleep(std::time::Duration::from_secs(2)).await;

        Ok(())
    }
}
