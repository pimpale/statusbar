use std::sync::Arc;
use std::time::Duration;

use auth_service_api::request::ApiKeyNewWithEmailProps;
use auth_service_api::response::ApiKey;
use derivative::Derivative;
use futures_util::{FutureExt, Sink, SinkExt, Stream, StreamExt, TryFutureExt};
use iced_native::command::Action;
use iced_native::keyboard::KeyCode;
use iced_native::{widget, Color};
use iced_winit::alignment;
use iced_winit::widget::{button, column, container, row, scrollable, text};
use iced_winit::Element;
use iced_winit::{theme, Command, Length};

use iced_wgpu::Renderer;

use once_cell::sync::Lazy;

use todoproxy_api::request::WebsocketInitMessage;
use todoproxy_api::{
    FinishedTask, LiveTask, StateSnapshot, TaskStatus, WebsocketOp, WebsocketOpKind,
};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite;

use crate::advanced_text_input;
use crate::program_runner::ProgramWithSubscription;
use crate::utils;
use crate::wm_hints;

// username and password text .valueboxes
static EMAIL_INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);
static PASSWORD_INPUT_ID: Lazy<advanced_text_input::Id> =
    Lazy::new(advanced_text_input::Id::unique);

// logged in boxes
static INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);
static ACTIVE_INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);

static AUTH_URL: &str = "http://localhost:8079/public";
static TODOPROXY_URL: &str = "ws://127.0.0.1:8080";

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

impl State {
    fn not_connected(api_key: String, error: Option<String>) -> State {
        State::NotConnected(NotConnectedState { api_key, error })
    }
}

#[derive(Debug)]
pub struct NotLoggedInState {
    email: String,
    password: String,
    view_password: bool,
    error: Option<String>,
}

#[derive(Debug)]
pub struct NotConnectedState {
    api_key: String,
    error: Option<String>,
}

type WebsocketStream = Box<
    dyn Stream<Item = Result<tungstenite::protocol::Message, tungstenite::error::Error>>
        + Unpin
        + Send,
>;

type WebsocketSink =
    Box<dyn Sink<tungstenite::protocol::Message, Error = tungstenite::error::Error> + Unpin + Send>;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectedState {
    api_key: String,
    #[derivative(Debug = "ignore")]
    websocket_recv: Arc<Mutex<WebsocketStream>>,
    #[derivative(Debug = "ignore")]
    websocket_send: Arc<Mutex<WebsocketSink>>,
    input_value: String,
    active_index: Option<usize>,
    snapshot: StateSnapshot,
}

enum ConnectedStateRecvKind {
    Nop,
    Op(WebsocketOp),
    Ping(Vec<u8>),
}

impl ConnectedState {
    // creates a command from an operation
    // (operation must be valid)
    fn wsop(&self, op: Op) -> Command<Message> {
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
                    id: self.snapshot.finished.first().unwrap().id.clone(),
                },
                Op::Pop(position, status) => WebsocketOpKind::FinishLiveTask {
                    id: self.snapshot.live[position].id.clone(),
                    status,
                },
                Op::Edit(position, value) => WebsocketOpKind::EditLiveTask {
                    id: self.snapshot.live[position].id.clone(),
                    value,
                },
                Op::Move(del, ins) => WebsocketOpKind::MvLiveTask {
                    id_del: self.snapshot.live[del].id.clone(),
                    id_ins: self.snapshot.live[ins].id.clone(),
                },
            },
        };

        let wsop_text = serde_json::to_string(&wsop).unwrap();

        Todos::send(
            self.websocket_send.clone(),
            tungstenite::protocol::Message::Text(wsop_text),
        )
    }

    fn handle_recv(
        &self,
        result: Option<Result<tungstenite::protocol::Message, String>>,
    ) -> Result<ConnectedStateRecvKind, String> {
        match result {
            Some(Ok(msg)) => match msg {
                tungstenite::Message::Text(msg) => serde_json::from_str(&msg)
                    .map(|v| ConnectedStateRecvKind::Op(v))
                    .map_err(report_serde_error),
                tungstenite::Message::Ping(data) => Ok(ConnectedStateRecvKind::Ping(data)),
                tungstenite::Message::Close(f) => Err(match f {
                    Some(f) => format!("connection closed: {}", f.reason),
                    None => String::from("connection closed unexpectedly"),
                }),
                _ => Ok(ConnectedStateRecvKind::Nop),
            },
            Some(Err(e)) => Err(e),
            None => Err(String::from("Lost connection")),
        }
    }
}

impl Default for NotLoggedInState {
    fn default() -> Self {
        NotLoggedInState {
            email: String::new(),
            password: String::new(),
            view_password: false,
            error: None,
        }
    }
}

#[derive(Clone, Derivative)]
#[derivative(Debug)]
pub enum Message {
    EventOccurred(iced_native::Event),
    // Focus
    FocusDock,
    UnfocusDock,
    // change dock
    ExpandDock,
    CollapseDock,

    // not logged in page
    EditEmail(String),
    SubmitEmail,
    EditPassword(String),
    SubmitPassword,
    TogglePasswordView,
    AttemptLogin,
    LoginAttemptComplete(Result<ApiKey, String>),
    ConnectAttemptComplete(
        #[derivative(Debug = "ignore")]
        Result<(Arc<Mutex<WebsocketSink>>, Arc<Mutex<WebsocketStream>>), String>,
    ),
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
    WebsocketRecvComplete(Option<Result<tungstenite::protocol::Message, String>>),
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

    fn next_widget() -> Command<Message> {
        Command::single(Action::Widget(widget::Action::new(
            widget::operation::focusable::focus_next(),
        )))
    }

    fn attempt_connect() -> Command<Message> {
        Command::single(Action::Future(Box::pin(async {
            Message::ConnectAttemptComplete(
                tokio_tungstenite::connect_async(format!("{}/ws/task_updates", TODOPROXY_URL))
                    .await
                    .map_err(report_tungstenite_error)
                    .map(|(w, _)| {
                        let (sink, stream) = w.split();
                        let sink: WebsocketSink = Box::new(sink);
                        let stream: WebsocketStream = Box::new(stream);
                        (Arc::new(Mutex::new(sink)), Arc::new(Mutex::new(stream)))
                    }),
            )
        })))
    }

    fn send(
        sink: Arc<Mutex<WebsocketSink>>,
        msg: tungstenite::protocol::Message,
    ) -> Command<Message> {
        Command::single(Action::Future(Box::pin(async move {
            Message::WebsocketSendComplete(
                sink.lock()
                    .await
                    .send(msg)
                    .await
                    .map_err(report_tungstenite_error),
            )
        })))
    }

    fn recv(stream: Arc<Mutex<WebsocketStream>>) -> Command<Message> {
        Command::single(Action::Future(Box::pin(async move {
            Message::WebsocketRecvComplete(
                stream
                    .lock()
                    .await
                    .next()
                    .await
                    .map(|x| x.map_err(report_tungstenite_error)),
            )
        })))
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
                iced_native::Event::Keyboard(iced_native::keyboard::Event::KeyPressed {
                    key_code: KeyCode::Tab,
                    ..
                }) => Todos::next_widget(),
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
                self.focused = false;
                if self.expanded {
                    wm_hints::ungrab_keyboard(&self.wm_state).unwrap();
                }
                Command::none()
            }
            Message::ExpandDock => {
                self.expanded = true;

                // grab keyboard focus
                if self.focused {
                    wm_hints::grab_keyboard(&self.wm_state).unwrap();
                }

                let command = match self.state {
                    State::NotLoggedIn(_) => advanced_text_input::focus(EMAIL_INPUT_ID.clone()),
                    State::Connected(_) => advanced_text_input::focus(INPUT_ID.clone()),
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
                    State::Connected(ref mut state) => {
                        state.input_value = String::new();
                        state.active_index = None;
                    }
                    _ => {}
                }

                iced_winit::window::resize(1, 50)
            }
            Message::EditInput(value) => {
                match self.state {
                    State::Connected(ref mut state) => {
                        state.input_value = value;
                    }
                    _ => {}
                }
                Command::none()
            }
            Message::SubmitInput => match self.state {
                State::Connected(ref mut state) => {
                    let val = std::mem::take(&mut state.input_value);
                    match val.split_once(" ").map(|x| x.0).unwrap_or(val.as_str()) {
                        "q" => Command::single(Action::Future(Box::pin(async {
                            Message::CollapseDock
                        }))),
                        "x" => panic!(),
                        "ps" => match state.snapshot.live.len() {
                            0 => Command::none(),
                            _ => state.wsop(Op::Pop(0, TaskStatus::Succeeded)),
                        }
                        "pf" => match state.snapshot.live.len() {
                            0 => Command::none(),
                            _ => state.wsop(Op::Pop(0, TaskStatus::Failed)),
                        }
                        "po" => match state.snapshot.live.len() {
                            0 => Command::none(),
                            _ => state.wsop(Op::Pop(0, TaskStatus::Obsoleted)),
                        }
                        "r" => {
                            if !state.snapshot.finished.is_empty() {
                                state.wsop(Op::RestoreFinished)
                            } else {
                                Command
                        }
                        "mv" => {
                            if let Ok((i, j)) = sscanf::scanf!(val, "mv {} {}", usize, usize) {
                                if i < state.snapshot.live.len() && j < state.snapshot.live.len() {
                                    state.wsop(Op::Move(i, j))
                                } else {
                                    Command::none()
                                }
                            } else if let Ok(i) = sscanf::scanf!(val, "mv {}", usize) {
                                if i < state.snapshot.live.len() {
                                    state.wsop(Op::Move(i, 0))
                                } else {
                                    Command::none()
                                }
                            } else if val == "mv" {
                                if state.snapshot.live.len() >= 2 {
                                    state.wsop(Op::Move(0, 1))
                                } else {
                                    Command::none()
                                }
                            } else {
                                Command::none()
                            }
                        }
                        _ => state.wsop(Op::NewLive(val, 0)),
                    }
                }
                _ => Command::none(),
            },
            Message::EditActive(value) => {
                match self.state {
                    State::Connected(ref mut state) => {
                        state.snapshot.live[state.active_index.unwrap()].value = value;
                    }
                    _ => {}
                }
                Command::none()
            }
            Message::SetActive(a) => match self.state {
                State::Connected(ref mut state) => {
                    let command = match state.active_index {
                        Some(position) => state.wsop(Op::Edit(
                            position,
                            state.snapshot.live[position].value.clone(),
                        )),
                        None => Command::none(),
                    };
                    state.active_index = a;
                    Command::batch([
                        command,
                        match a {
                            Some(_) => advanced_text_input::focus(ACTIVE_INPUT_ID.clone()),
                            None => Command::none(),
                        },
                    ])
                }
                _ => Command::none(),
            },
            Message::Op(op) => match self.state {
                State::Connected(ref mut state) => state.wsop(op),
                _ => Command::none(),
            },
            Message::EditEmail(val) => match self.state {
                State::NotLoggedIn(ref mut state) => {
                    state.email = val;
                    state.error = None;
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::SubmitEmail => match self.state {
                State::NotLoggedIn(_) => Todos::next_widget(),
                _ => Command::none(),
            },
            Message::EditPassword(val) => match self.state {
                State::NotLoggedIn(ref mut state) => {
                    state.password = val;
                    state.error = None;
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::SubmitPassword => match self.state {
                State::NotLoggedIn(ref state) => {
                    if !state.email.is_empty() && !state.password.is_empty() {
                        Command::single(Action::Future(Box::pin(async { Message::AttemptLogin })))
                    } else {
                        Todos::next_widget()
                    }
                }
                _ => Command::none(),
            },
            Message::TogglePasswordView => match self.state {
                State::NotLoggedIn(ref mut state) => {
                    state.view_password = !state.view_password;
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::AttemptLogin => match self.state {
                State::NotLoggedIn(ref state) => {
                    let auth_base_url = String::from(AUTH_URL);
                    let email = state.email.clone();
                    let password = state.password.clone();
                    let duration = Duration::from_secs(24 * 60 * 60).as_millis() as i64;
                    Command::single(Action::Future(Box::pin(async move {
                        Message::LoginAttemptComplete(
                            api_key_new_with_email(auth_base_url, email, password, duration).await,
                        )
                    })))
                }
                _ => Command::none(),
            },
            Message::LoginAttemptComplete(res) => match self.state {
                State::NotLoggedIn(ref mut state) => {
                    match res {
                        // if ok then transition state to NotConnected and try to connect
                        Ok(ApiKey {
                            key: Some(api_key), ..
                        }) => {
                            self.state = State::not_connected(api_key, None);
                            // we need to now try to initialize the websocket connection
                            Todos::attempt_connect()
                        }
                        Ok(_) => {
                            state.error = Some(String::from("No ApiKey returned"));
                            Command::none()
                        }
                        Err(e) => {
                            state.error = Some(e);
                            Command::none()
                        }
                    }
                }
                _ => Command::none(),
            },
            Message::RetryConnect => match self.state {
                State::NotConnected(ref mut state) => {
                    state.error = None;
                    Todos::attempt_connect()
                }
                _ => Command::none(),
            },
            Message::ConnectAttemptComplete(result) => match self.state {
                State::NotConnected(ref mut state) => match result {
                    Ok((sink, stream)) => {
                        let api_key = state.api_key.clone();
                        self.state = State::Connected(ConnectedState {
                            api_key: api_key.clone(),
                            websocket_recv: stream.clone(),
                            websocket_send: sink.clone(),
                            input_value: String::new(),
                            active_index: None,
                            snapshot: StateSnapshot {
                                live: vec![].into(),
                                finished: vec![],
                            },
                        });

                        let msg = tungstenite::protocol::Message::Text(
                            serde_json::to_string(&WebsocketInitMessage { api_key }).unwrap(),
                        );

                        // send the auth initializer and begin listening
                        Command::batch([Todos::send(sink, msg), Todos::recv(stream)])
                    }
                    Err(e) => {
                        state.error = Some(e);
                        Command::none()
                    }
                },
                _ => Command::none(),
            },
            Message::WebsocketSendComplete(result) => match self.state {
                State::Connected(ref state) => match result {
                    Ok(()) => Command::none(),
                    // on any send error, it's probably because the connection died
                    // in this case, return back to NotConnected
                    Err(e) => {
                        self.state = State::not_connected(state.api_key.clone(), Some(e));
                        // we probably don't want to immediately try recommecting, let's let the user press the retry button
                        Command::none()
                    }
                },
                _ => Command::none(),
            },
            Message::WebsocketRecvComplete(result) => match self.state {
                State::Connected(ref mut state) => match state.handle_recv(result) {
                    Ok(v) => Command::batch([
                        Todos::recv(state.websocket_recv.clone()),
                        match v {
                            ConnectedStateRecvKind::Nop => Command::none(),
                            ConnectedStateRecvKind::Ping(data) => Todos::send(
                                state.websocket_send.clone(),
                                tungstenite::protocol::Message::Pong(data),
                            ),
                            ConnectedStateRecvKind::Op(WebsocketOp { kind, .. }) => {
                                apply_operation(&mut state.snapshot, kind);
                                Command::none()
                            }
                        },
                    ]),
                    Err(e) => {
                        self.state = State::not_connected(state.api_key.clone(), Some(e));
                        Command::none()
                    }
                },
                _ => Command::none(),
            },
        }
    }

    fn view(&self) -> Element<Message, Renderer> {
        match self {
            Self {
                state: State::NotLoggedIn(_),
                expanded: false,
                ..
            } => button(
                text("Click to Log In")
                    .horizontal_alignment(alignment::Horizontal::Center)
                    .vertical_alignment(alignment::Vertical::Center)
                    .height(Length::Fill)
                    .width(Length::Fill),
            )
            .style(theme::Button::Text)
            .height(Length::Fill)
            .width(Length::Fill)
            .on_press(Message::ExpandDock)
            .into(),
            Self {
                state:
                    State::NotLoggedIn(NotLoggedInState {
                        email,
                        password,
                        view_password,
                        error,
                    }),
                expanded: true,
                ..
            } => {
                let email_input =
                    advanced_text_input::AdvancedTextInput::new("Email", email, Message::EditEmail)
                        .id(EMAIL_INPUT_ID.clone())
                        .on_submit(Message::SubmitEmail);

                let mut password_input = advanced_text_input::AdvancedTextInput::new(
                    "Password",
                    password,
                    Message::EditPassword,
                )
                .id(PASSWORD_INPUT_ID.clone())
                .on_submit(Message::SubmitPassword);

                if !view_password {
                    password_input = password_input.password();
                }

                let error = match error {
                    Some(error) => text(error).style(Color::from([1.0, 0.0, 0.0])),
                    None => text(""),
                };

                let submit_button = button("Submit").on_press(Message::AttemptLogin);

                row(vec![
                    column(vec![
                        button("Collapse")
                            .on_press(Message::CollapseDock)
                            .width(Length::Shrink)
                            .into(),
                        button(if *view_password {
                            "Hide Password"
                        } else {
                            "View Password"
                        })
                        .on_press(Message::TogglePasswordView)
                        .width(Length::Shrink)
                        .into(),
                    ])
                    .spacing(10)
                    .width(Length::Shrink)
                    .into(),
                    column(vec![
                        email_input.into(),
                        password_input.into(),
                        submit_button.into(),
                        error.into(),
                    ])
                    .spacing(10)
                    .width(Length::Shrink)
                    .into(),
                ])
                .spacing(10)
                .padding(10)
                .into()
            }
            Self {
                state: State::NotConnected(NotConnectedState { error, .. }),
                expanded: false,
                ..
            } => {
                let text = match error {
                    Some(e) => text(e).style(Color::from_rgb(1.0, 0.0, 0.0)),
                    None => text("Connecting..."),
                };
                // button should fill whole screen
                button(
                    text.horizontal_alignment(alignment::Horizontal::Center)
                        .vertical_alignment(alignment::Vertical::Center)
                        .height(Length::Fill)
                        .width(Length::Fill),
                )
                .style(theme::Button::Text)
                .height(Length::Fill)
                .width(Length::Fill)
                .on_press(Message::ExpandDock)
                .into()
            }
            Self {
                state: State::NotConnected(NotConnectedState { error, .. }),
                expanded: true,
                ..
            } => row(vec![
                button("Collapse").on_press(Message::CollapseDock).into(),
                column(match error {
                    Some(error) => vec![
                        text(error)
                            .style(Color::from([1.0, 0.0, 0.0]))
                            .horizontal_alignment(alignment::Horizontal::Center)
                            .into(),
                        button("Retry").on_press(Message::RetryConnect).into(),
                    ],
                    None => vec![text("Connecting...").into()],
                })
                .spacing(10)
                .width(Length::Shrink)
                .into(),
            ])
            .spacing(10)
            .padding(10)
            .into(),
            Self {
                state: State::Connected(ConnectedState { snapshot, .. }),
                expanded: false,
                ..
            } => match snapshot.live.front() {
                None => container(button("Click to Add Task").on_press(Message::ExpandDock))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x()
                    .center_y()
                    .padding(10)
                    .into(),
                Some(LiveTask { value, .. }) => container(
                    row(vec![
                        button("Task Succeeded")
                            .height(Length::Fill)
                            .style(theme::Button::Positive)
                            .on_press(Message::Op(Op::Pop(0, TaskStatus::Succeeded)))
                            .into(),
                        button(text(value).horizontal_alignment(alignment::Horizontal::Center))
                            .height(Length::Fill)
                            .width(Length::Fill)
                            .style(theme::Button::Text)
                            .on_press(Message::ExpandDock)
                            .into(),
                        button("Task Failed")
                            .height(Length::Fill)
                            .style(theme::Button::Destructive)
                            .on_press(Message::Op(Op::Pop(0, TaskStatus::Failed)))
                            .into(),
                        button("Task Obsoleted")
                            .height(Length::Fill)
                            .style(theme::Button::Secondary)
                            .on_press(Message::Op(Op::Pop(0, TaskStatus::Obsoleted)))
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
            Self {
                state:
                    State::Connected(ConnectedState {
                        input_value,
                        snapshot: StateSnapshot { live, finished },
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
                .on_submit(Message::SubmitInput);

                let tasks: Element<_, Renderer> = if live.len() > 0 {
                    column(
                        live.iter()
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
                                            &task.value,
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
                                        button(text(&task.value))
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
        }
    }

    fn handle_uncaptured_events(&self, events: Vec<iced_native::Event>) -> Vec<Self::Message> {
        events.into_iter().map(Message::EventOccurred).collect()
    }
}

async fn api_key_new_with_email(
    auth_base_url: String,
    email: String,
    password: String,
    duration: i64,
) -> Result<ApiKey, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api_key/new_with_email", auth_base_url))
        .json(&ApiKeyNewWithEmailProps {
            email,
            password,
            duration,
        })
        .send()
        .await
        .map_err(report_reqwest_error)?;

    if resp.status().as_u16() == 200 {
        Ok(resp.json().await.map_err(report_reqwest_error)?)
    } else {
        Err(resp.text().await.map_err(report_reqwest_error)?)
    }
}

fn apply_operation(
    StateSnapshot {
        ref mut finished,
        ref mut live,
    }: &mut StateSnapshot,
    op: WebsocketOpKind,
) {
    match op {
        WebsocketOpKind::OverwriteState(s) => {
            *live = s.live;
            *finished = s.finished;
        }
        WebsocketOpKind::InsLiveTask {
            value,
            id,
            position,
        } => {
            if position <= live.len() {
                live.insert(position, LiveTask { id, value });
            }
        }
        WebsocketOpKind::RestoreFinishedTask { id } => {
            // if it was found in the finished list, push it to the front
            if let Some(position) = finished.iter().position(|x| x.id == id) {
                let FinishedTask { id, value, .. } = finished.remove(position);
                live.push_front(LiveTask { id, value });
            }
        }
        WebsocketOpKind::EditLiveTask { id, value } => {
            for x in live.iter_mut() {
                if x.id == id {
                    x.value = value;
                    break;
                }
            }
        }
        WebsocketOpKind::DelLiveTask { id } => {
            live.retain(|x| x.id != id);
        }
        WebsocketOpKind::MvLiveTask { id_ins, id_del } => {
            let ins_pos = live.iter().position(|x| x.id == id_ins);
            let del_pos = live.iter().position(|x| x.id == id_del);

            if let (Some(mut ins_pos), Some(del_pos)) = (ins_pos, del_pos) {
                let removed = live.remove(del_pos).unwrap();
                live.insert(ins_pos, removed);
            }
        }
        WebsocketOpKind::FinishLiveTask { id, status } => {
            if let Some(pos_in_live) = live.iter().position(|x| x.id == id) {
                finished.push(FinishedTask {
                    id,
                    value: live.remove(pos_in_live).unwrap().value,
                    status,
                });
            }
        }
    }
}

pub fn report_reqwest_error(e: reqwest::Error) -> String {
    log::error!("{}", e);
    e.to_string()
}

pub fn report_serde_error(e: serde_json::Error) -> String {
    log::error!("{}", e);
    e.to_string()
}

pub fn report_tungstenite_error(e: tungstenite::error::Error) -> String {
    log::error!("{}", e);
    e.to_string()
}
