use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use auth_service_api::request::ApiKeyNewWithEmailProps;
use auth_service_api::response::ApiKey;
use derivative::Derivative;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use iced_native::command::Action;
use iced_native::keyboard::KeyCode;
use iced_native::{widget, Color};
use iced_winit::alignment;
use iced_winit::widget::{button, column, container, row, scrollable, text};
use iced_winit::Element;
use iced_winit::{theme, Command, Length};

use iced_wgpu::Renderer;

use once_cell::sync::Lazy;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use todoproxy_api::response::Info;
use todoproxy_api::{
    FinishedTask, LiveTask, StateSnapshot, TaskStatus, WebsocketOp, WebsocketOpKind,
};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite;

use crate::program_runner::ProgramWithSubscription;
use crate::utils;
use crate::wm_hints;
use crate::{advanced_text_input, xdg_manager};

// username and password text .valueboxes
static EMAIL_INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);
static PASSWORD_INPUT_ID: Lazy<advanced_text_input::Id> =
    Lazy::new(advanced_text_input::Id::unique);

// logged in boxes
static INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);
static ACTIVE_INPUT_ID: Lazy<advanced_text_input::Id> = Lazy::new(advanced_text_input::Id::unique);

static CONFIG_FILENAME: &'static str = "config.json";
static CACHE_FILENAME: &'static str = "cache.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct TodosConfig {
    server_api_url: String,
}

impl Default for TodosConfig {
    fn default() -> Self {
        TodosConfig {
            server_api_url: String::from("http://localhost:8080/public/"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TodosCache {
    // used to check if the server_api_url has changed in config (and thus we need to reset)
    server_api_url: String,
    api_key: String,
}

#[derive(Debug)]
pub struct Todos {
    server_api_url: Url,
    wm_state: wm_hints::WmHintsState,
    grabbed: bool,
    focused: bool,
    expanded: bool,
    state: State,
    nocache: bool,
}

#[derive(Debug)]
pub enum State {
    NotLoggedIn(NotLoggedInState),
    Restored(RestoredState),
    NotConnected(NotConnectedState),
    Connected(ConnectedState),
}

impl State {
    fn from_cache(api_key: String) -> State {
        State::Restored(RestoredState { api_key })
    }

    fn not_connected(api_key: String, error: Option<String>) -> State {
        State::NotConnected(NotConnectedState { api_key, error })
    }

    fn not_logged_in() -> State {
        State::NotLoggedIn(NotLoggedInState {
            email: String::new(),
            password: String::new(),
            view_password: false,
            error: None,
        })
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

#[derive(Debug)]
pub struct RestoredState {
    api_key: String,
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
    active_id_val: Option<(String, String)>,
    snapshot: StateSnapshot,
    show_finished: bool,
}

enum ConnectedStateRecvKind {
    Nop,
    Op(WebsocketOp),
    Ping(Vec<u8>),
}

enum ConnectionCloseKind {
    Unauthorized,
    Other(String),
}

#[derive(Debug, Clone)]
pub enum Op {
    NewLive(String),
    RestoreFinished(String),
    Pop(String, TaskStatus),
    Edit(String, String),
    Move(String, String),
    Rev(String, String),
}

impl ConnectedState {
    // creates a command from an operation
    // (operation must be valid)
    fn wsop(&self, op: Op) -> Command<Message> {
        // create op
        let wsop = WebsocketOp {
            alleged_time: utils::current_time_millis(),
            kind: match op {
                Op::NewLive(value) => WebsocketOpKind::InsLiveTask {
                    value,
                    id: utils::random_string(),
                },
                Op::RestoreFinished(id) => WebsocketOpKind::RestoreFinishedTask { id },
                Op::Pop(id, status) => WebsocketOpKind::FinishLiveTask { id, status },
                Op::Edit(id, value) => WebsocketOpKind::EditLiveTask { id, value },
                Op::Move(id_del, id_ins) => WebsocketOpKind::MvLiveTask { id_del, id_ins },
                Op::Rev(id1, id2) => WebsocketOpKind::RevLiveTask { id1, id2 },
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
    ) -> Result<ConnectedStateRecvKind, ConnectionCloseKind> {
        match result {
            Some(Ok(msg)) => match msg {
                tungstenite::Message::Text(msg) => serde_json::from_str(&msg)
                    .map(|v| ConnectedStateRecvKind::Op(v))
                    .map_err(report_serde_error)
                    .map_err(ConnectionCloseKind::Other),
                tungstenite::Message::Ping(data) => Ok(ConnectedStateRecvKind::Ping(data)),
                tungstenite::Message::Close(f) => Err(match f {
                    Some(f) => match f.reason.to_string().as_str() {
                        "Unauthorized" => ConnectionCloseKind::Unauthorized,
                        _ => ConnectionCloseKind::Other(f.reason.to_string()),
                    },
                    None => {
                        ConnectionCloseKind::Other(String::from("connection closed unexpectedly"))
                    }
                }),
                _ => Ok(ConnectedStateRecvKind::Nop),
            },
            Some(Err(e)) => Err(ConnectionCloseKind::Other(e)),
            None => Err(ConnectionCloseKind::Other(String::from("Lost connection"))),
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
    LogOut,
    // connected page
    EditInput(String),
    SubmitInput,
    EditActive(String),
    SetActive(Option<String>),
    Op(Op),
    ToggleFinished,
    // Websocket Interactions
    WebsocketSendComplete(Result<(), String>),
    WebsocketRecvComplete(Option<Result<tungstenite::protocol::Message, String>>),
    // Check timeout
    CheckWebsocketTimeout,
}

impl Todos {
    pub fn new(
        wm_state: wm_hints::WmHintsState,
        nocache: bool,
        remote_url: Option<String>,
    ) -> Result<Todos, Box<dyn std::error::Error>> {
        let server_api_url = match remote_url {
            Some(url) => Url::parse(&url)?,
            None => {
                // try to read config
                let config =
                    xdg_manager::get_or_create_config::<TodosConfig>(CONFIG_FILENAME).unwrap();

                Url::parse(&config.server_api_url)?
            }
        };

        // throw error if not https or http
        match server_api_url.scheme() {
            "https" | "http" => {}
            _ => Err("invalid url")?,
        }

        let state = if nocache {
            State::not_logged_in()
        } else {
            let cache = xdg_manager::load_cache_if_exists::<TodosCache>(CACHE_FILENAME).unwrap();
            match cache {
                Some(cache) if cache.server_api_url == server_api_url.as_str() => {
                    State::from_cache(cache.api_key)
                }
                _ => State::not_logged_in(),
            }
        };

        Ok(Todos {
            nocache,
            server_api_url,
            wm_state,
            grabbed: false,
            expanded: false,
            focused: false,
            state,
        })
    }

    fn next_widget() -> Command<Message> {
        Command::single(Action::Widget(widget::Action::new(
            widget::operation::focusable::focus_next(),
        )))
    }

    fn attempt_connect(&self, api_key: &str) -> Command<Message> {
        let mut ws_task_updates_url = self.server_api_url.join("ws/task_updates").unwrap();

        if ws_task_updates_url.scheme() == "https" {
            ws_task_updates_url.set_scheme("wss").unwrap();
        } else {
            ws_task_updates_url.set_scheme("ws").unwrap();
        }

        // set parameters
        ws_task_updates_url.set_query(Some(format!("api_key={}", api_key).as_str()));

        Command::single(Action::Future(Box::pin(async move {
            Message::ConnectAttemptComplete(
                tokio_tungstenite::connect_async(ws_task_updates_url)
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

    fn delay_message(duration: Duration, m: Message) -> Command<Message> {
        Command::single(Action::Future(Box::pin(async move {
            tokio::time::sleep(duration).await;
            m
        })))
    }
}

impl ProgramWithSubscription for Todos {
    type Message = Message;
    type Renderer = Renderer;

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::EventOccurred(event) => match event {
                // grab keyboard focus on cursor enter
                iced_native::Event::Mouse(iced_native::mouse::Event::CursorEntered) => {
                    Command::single(Action::Future(Box::pin(async { Message::FocusDock })))
                }
                // release keyboard focus on cursor exit
                iced_native::Event::Mouse(iced_native::mouse::Event::CursorLeft) => {
                    Command::single(Action::Future(Box::pin(async { Message::UnfocusDock })))
                }
                iced_native::Event::Mouse(iced_native::mouse::Event::CursorMoved { .. }) => {
                    Command::single(Action::Future(Box::pin(async { Message::FocusDock })))
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
                    if !self.grabbed {
                        match wm_hints::grab_keyboard(&self.wm_state).map_err(report_wmhints_error)
                        {
                            Ok(_) => self.grabbed = true,
                            _ => {}
                        }
                    }
                }
                Command::none()
            }
            Message::UnfocusDock => {
                self.focused = false;
                if self.grabbed {
                    match wm_hints::ungrab_keyboard(&self.wm_state).map_err(report_wmhints_error) {
                        Ok(_) => self.grabbed = false,
                        _ => {}
                    }
                }
                Command::none()
            }
            Message::ExpandDock => {
                self.expanded = true;

                // grab keyboard focus
                if self.focused {
                    match wm_hints::grab_keyboard(&self.wm_state).map_err(report_wmhints_error) {
                        Ok(_) => self.grabbed = true,
                        _ => {}
                    }
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
                if self.grabbed {
                    match wm_hints::ungrab_keyboard(&self.wm_state).map_err(report_wmhints_error) {
                        Ok(_) => self.grabbed = false,
                        _ => {}
                    }
                }
                match self.state {
                    State::Connected(ref mut state) => {
                        state.input_value = String::new();
                        state.active_id_val = None;
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
                        "x" => panic!(),
                        "c" => Command::single(Action::Future(Box::pin(async {
                            Message::CollapseDock
                        }))),
                        "t" => Command::single(Action::Future(Box::pin(async {
                            Message::ToggleFinished
                        }))),
                        "s" => match state.snapshot.live.front() {
                            None => Command::none(),
                            Some(task) => {
                                state.wsop(Op::Pop(task.id.clone(), TaskStatus::Succeeded))
                            }
                        },
                        "f" => match state.snapshot.live.front() {
                            None => Command::none(),
                            Some(task) => state.wsop(Op::Pop(task.id.clone(), TaskStatus::Failed)),
                        },
                        "o" => match state.snapshot.live.front() {
                            None => Command::none(),
                            Some(task) => {
                                state.wsop(Op::Pop(task.id.clone(), TaskStatus::Obsoleted))
                            }
                        },
                        "r" => {
                            let f = &state.snapshot.finished;
                            if let Ok(i) = sscanf::scanf!(val, "r {}", usize) {
                                if i < f.len() {
                                    state.wsop(Op::RestoreFinished(f[i].id.clone()))
                                } else {
                                    Command::none()
                                }
                            } else if val == "r" {
                                if 0 < f.len() {
                                    state.wsop(Op::RestoreFinished(f[0].id.clone()))
                                } else {
                                    Command::none()
                                }
                            } else {
                                Command::none()
                            }
                        }
                        "q" => {
                            let l = &state.snapshot.live;
                            if let Ok(i) = sscanf::scanf!(val, "q {}", usize) {
                                match (l.get(i), l.back()) {
                                    (Some(f), Some(b)) if l.len() > 1 => {
                                        state.wsop(Op::Move(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else if val == "q" {
                                match (l.front(), l.back()) {
                                    (Some(f), Some(b)) if l.len() > 1 => {
                                        state.wsop(Op::Move(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else {
                                Command::none()
                            }
                        }
                        "mv" => {
                            let l = &state.snapshot.live;
                            if let Ok((i, j)) = sscanf::scanf!(val, "mv {} {}", usize, usize) {
                                match (l.get(i), l.get(j)) {
                                    (Some(f), Some(b)) if i != j => {
                                        state.wsop(Op::Move(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else if let Ok(i) = sscanf::scanf!(val, "mv {}", usize) {
                                match (l.get(i), l.front()) {
                                    (Some(f), Some(b)) if l.len() > 1 => {
                                        state.wsop(Op::Move(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else {
                                Command::none()
                            }
                        }
                        "rev" => {
                            let l = &state.snapshot.live;
                            if let Ok((i, j)) = sscanf::scanf!(val, "rev {} {}", usize, usize) {
                                match (l.get(i), l.get(j)) {
                                    (Some(f), Some(b)) if i != j => {
                                        state.wsop(Op::Rev(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else if let Ok(i) = sscanf::scanf!(val, "rev {}", usize) {
                                match (l.get(i), l.front()) {
                                    (Some(f), Some(b)) if l.len() > 1 => {
                                        state.wsop(Op::Rev(f.id.clone(), b.id.clone()))
                                    }
                                    _ => Command::none(),
                                }
                            } else {
                                Command::none()
                            }
                        }
                        _ => state.wsop(Op::NewLive(val)),
                    }
                }
                _ => Command::none(),
            },
            Message::EditActive(new_value) => {
                match self.state {
                    State::Connected(ref mut state) => match state.active_id_val {
                        Some((_, ref mut value)) => *value = new_value,
                        None => {}
                    },
                    _ => {}
                }
                Command::none()
            }
            Message::SetActive(a) => match self.state {
                State::Connected(ref mut state) => {
                    let edit_command = match state.active_id_val {
                        Some((ref id, ref value)) => {
                            state.wsop(Op::Edit(id.clone(), value.clone()))
                        }
                        None => Command::none(),
                    };
                    let focus_command = match a {
                        Some(_) => advanced_text_input::focus(ACTIVE_INPUT_ID.clone()),
                        None => Command::none(),
                    };

                    match a {
                        Some(id) => {
                            let value = state
                                .snapshot
                                .live
                                .iter()
                                .find(|x| x.id == id)
                                .unwrap()
                                .value
                                .clone();
                            state.active_id_val = Some((id, value))
                        }
                        None => state.active_id_val = None,
                    }

                    Command::batch([edit_command, focus_command])
                }
                _ => Command::none(),
            },
            Message::Op(op) => match self.state {
                State::Connected(ref mut state) => state.wsop(op),
                _ => Command::none(),
            },
            Message::LogOut => match self.state {
                State::Connected(_) | State::NotConnected(_) => {
                    if !self.nocache {
                        xdg_manager::delete_cache(CACHE_FILENAME).unwrap();
                    }
                    self.state = State::not_logged_in();
                    Command::none()
                }
                _ => Command::none(),
            },
            Message::ToggleFinished => match self.state {
                State::Connected(ref mut state) => {
                    state.show_finished = !state.show_finished;
                    Command::none()
                }
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
                    let server_api_url = self.server_api_url.clone();
                    let email = state.email.clone();
                    let password = state.password.clone();
                    let duration = Duration::from_secs(24 * 60 * 60).as_millis() as i64;
                    Command::single(Action::Future(Box::pin(async move {
                        Message::LoginAttemptComplete(
                            do_login(server_api_url, email, password, duration).await,
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
                            // save in cache file
                            if !self.nocache {
                                let cache = TodosCache {
                                    server_api_url: self.server_api_url.clone().into(),
                                    api_key: api_key.clone(),
                                };
                                xdg_manager::write_cache("cache.json", &cache).unwrap();
                            }

                            // we need to now try to initialize the websocket connection
                            let connect_attempt_result = self.attempt_connect(&api_key);
                            // switch state
                            self.state = State::not_connected(api_key, None);
                            // return the result
                            connect_attempt_result
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
                State::Restored(ref mut state) => {
                    // we need to now try to initialize the websocket connection
                    let api_key = state.api_key.clone();
                    self.state = State::not_connected(state.api_key.clone(), None);
                    self.attempt_connect(&api_key)
                }
                State::NotConnected(ref mut state) => {
                    state.error = None;
                    let api_key = &state.api_key.clone();
                    self.attempt_connect(api_key)
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
                            active_id_val: None,
                            snapshot: StateSnapshot {
                                live: VecDeque::new(),
                                finished: VecDeque::new(),
                            },
                            show_finished: false,
                        });

                        Command::batch([
                            // start recieving responses
                            Todos::recv(stream),
                            // focus the input bar
                            advanced_text_input::focus(INPUT_ID.clone()),
                        ])
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
                                apply_operation(
                                    &mut state.snapshot,
                                    &mut state.active_id_val,
                                    kind,
                                );
                                Command::none()
                            }
                        },
                    ]),
                    Err(ConnectionCloseKind::Unauthorized) => {
                        self.state = State::not_logged_in();
                        Command::none()
                    }
                    Err(ConnectionCloseKind::Other(e)) => {
                        self.state = State::not_connected(state.api_key.clone(), Some(e));
                        Command::none()
                    }
                },
                _ => Command::none(),
            },
            Message::PingTimedOut(data) => match self.state {
                State::Connected(ref mut state) => {
                    if state.pending_pings.contains(&data) {
                        // we didn't receive pong in time
                        self.state = State::not_connected(
                            state.api_key.clone(),
                            Some(String::from("websocket timed out")),
                        );
                    }
                    Command::none()
                }
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
                state: State::Restored(_),
                ..
            } => button(
                text("Resume Session")
                    .horizontal_alignment(alignment::Horizontal::Center)
                    .vertical_alignment(alignment::Vertical::Center)
                    .height(Length::Fill)
                    .width(Length::Fill),
            )
            .style(theme::Button::Text)
            .height(Length::Fill)
            .width(Length::Fill)
            .on_press(Message::RetryConnect)
            .into(),
            Self {
                state: State::NotConnected(NotConnectedState { error, .. }),
                expanded: false,
                ..
            } => {
                let text = match error {
                    Some(e) => text(e).style(Color::from_rgb(1.0, 0.0, 0.0)),
                    None => text("Connecting..."),
                };
                let open_button = button(
                    text.horizontal_alignment(alignment::Horizontal::Center)
                        .vertical_alignment(alignment::Vertical::Center)
                        .height(Length::Fill)
                        .width(Length::Fill),
                )
                .style(theme::Button::Text)
                .height(Length::Fill)
                .width(Length::Fill)
                .on_press(Message::ExpandDock)
                .into();

                if error.is_some() {
                    row(vec![
                        open_button,
                        button("Retry").on_press(Message::RetryConnect).into(),
                    ])
                    .spacing(10)
                    .padding(10)
                    .into()
                } else {
                    open_button
                }
            }
            Self {
                state: State::NotConnected(NotConnectedState { error, .. }),
                expanded: true,
                ..
            } => row(vec![
                column(vec![
                    button("Collapse").on_press(Message::CollapseDock).into(),
                    button("Log Out").on_press(Message::LogOut).into(),
                ])
                .spacing(10)
                .into(),
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
                Some(LiveTask { value, id }) => container(
                    row(vec![
                        button("Task Succeeded")
                            .height(Length::Fill)
                            .style(theme::Button::Positive)
                            .on_press(Message::Op(Op::Pop(id.clone(), TaskStatus::Succeeded)))
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
                            .on_press(Message::Op(Op::Pop(id.clone(), TaskStatus::Failed)))
                            .into(),
                        button("Task Obsoleted")
                            .height(Length::Fill)
                            .style(theme::Button::Secondary)
                            .on_press(Message::Op(Op::Pop(id.clone(), TaskStatus::Obsoleted)))
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
                        active_id_val,
                        show_finished,
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

                let tasks: Element<_, Renderer> = if !*show_finished {
                    if live.len() > 0 {
                        column(
                            live.iter()
                                .enumerate()
                                .map(|(i, task)| {
                                    let header = text(format!("{}|", i)).size(25);

                                    match active_id_val {
                                        Some((active_id, val)) if active_id == &task.id => {
                                            row(vec![
                                                header.into(),
                                                button("Task Succeeded")
                                                    .style(theme::Button::Positive)
                                                    .on_press(Message::Op(Op::Pop(
                                                        task.id.clone(),
                                                        TaskStatus::Succeeded,
                                                    )))
                                                    .into(),
                                                advanced_text_input::AdvancedTextInput::new(
                                                    "Edit Task",
                                                    val,
                                                    Message::EditActive,
                                                )
                                                .id(ACTIVE_INPUT_ID.clone())
                                                .on_submit(Message::SetActive(None))
                                                .into(),
                                                button("Task Failed")
                                                    .style(theme::Button::Destructive)
                                                    .on_press(Message::Op(Op::Pop(
                                                        task.id.clone(),
                                                        TaskStatus::Failed,
                                                    )))
                                                    .into(),
                                                button("Task Obsoleted")
                                                    .style(theme::Button::Secondary)
                                                    .on_press(Message::Op(Op::Pop(
                                                        task.id.clone(),
                                                        TaskStatus::Obsoleted,
                                                    )))
                                                    .into(),
                                            ])
                                            .spacing(10)
                                            .into()
                                        }
                                        _ => row(vec![
                                            header.into(),
                                            button(text(&task.value))
                                                .on_press(Message::SetActive(Some(task.id.clone())))
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
                    }
                } else {
                    column(
                        finished
                            .iter()
                            .enumerate()
                            .map(|(i, task)| {
                                row(vec![
                                    text(format!("{}|", i)).size(25).into(),
                                    match task.status {
                                        TaskStatus::Succeeded => {
                                            text("SUCCEEDED").style(Color::from_rgb(0.0, 1.0, 0.0))
                                        }
                                        TaskStatus::Failed => {
                                            text("FAILED").style(Color::from_rgb(1.0, 0.0, 0.0))
                                        }
                                        TaskStatus::Obsoleted => {
                                            text("OBSOLETED").style(Color::from_rgb(0.7, 0.7, 0.7))
                                        }
                                    }
                                    .width(80.0)
                                    .size(20)
                                    .into(),
                                    text(&task.value).into(),
                                ])
                                .spacing(10)
                                .width(Length::Fill)
                                .into()
                            })
                            .collect(),
                    )
                    // pad right to avoid clipping scrollable
                    .padding([0, 15, 0, 0])
                    .into()
                };

                row(vec![
                    column(vec![
                        button("Collapse").on_press(Message::CollapseDock).into(),
                        button(match show_finished {
                            true => "Show Live Tasks",
                            false => "Show Finished Tasks",
                        })
                        .on_press(Message::ToggleFinished)
                        .into(),
                        button("Log Out").on_press(Message::LogOut).into(),
                    ])
                    .spacing(10)
                    .into(),
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

async fn do_login(
    server_api_url: Url,
    email: String,
    password: String,
    duration: i64,
) -> Result<ApiKey, String> {
    let client = reqwest::Client::new();

    // get info
    let resp = client
        .get(server_api_url.join("info").map_err(report_url_error)?)
        .send()
        .await
        .map_err(report_reqwest_error)?;

    let info: Info = match resp.status().as_u16() {
        200..=299 => Ok(resp.json().await.map_err(report_reqwest_error)?),
        status => Err(format!(
            "{}: {}",
            status,
            resp.text().await.map_err(report_reqwest_error)?
        )),
    }?;

    let auth_pub_api_href = Url::parse(&info.auth_pub_api_href).map_err(report_url_error)?;

    // get api key
    let resp = client
        .post(
            auth_pub_api_href
                .join("api_key/new_with_email")
                .map_err(report_url_error)?,
        )
        .json(&ApiKeyNewWithEmailProps {
            email,
            password,
            duration,
        })
        .send()
        .await
        .map_err(report_reqwest_error)?;

    match resp.status().as_u16() {
        200..=299 => Ok(resp.json().await.map_err(report_reqwest_error)?),
        status => Err(format!(
            "{}: {}",
            status,
            resp.text().await.map_err(report_reqwest_error)?
        )),
    }
}

fn apply_operation(
    StateSnapshot {
        ref mut finished,
        ref mut live,
    }: &mut StateSnapshot,
    active_id_val: &mut Option<(String, String)>,
    op: WebsocketOpKind,
) {
    match op {
        WebsocketOpKind::OverwriteState(s) => {
            *active_id_val = None;
            *live = s.live;
            *finished = s.finished;
        }
        WebsocketOpKind::InsLiveTask { value, id } => {
            live.push_front(LiveTask { id, value });
        }
        WebsocketOpKind::RestoreFinishedTask { id } => {
            // if it was found in the finished list, push it to the front
            if let Some(position) = finished.iter().position(|x| x.id == id) {
                let FinishedTask { id, value, .. } = finished.remove(position).unwrap();
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
            if let Some((active_id, _)) = active_id_val {
                if &id == active_id {
                    *active_id_val = None;
                }
            }
        }
        WebsocketOpKind::MvLiveTask { id_ins, id_del } => {
            let ins_pos = live.iter().position(|x| x.id == id_ins);
            let del_pos = live.iter().position(|x| x.id == id_del);

            if let (Some(ins_pos), Some(del_pos)) = (ins_pos, del_pos) {
                let removed = live.remove(del_pos).unwrap();
                live.insert(ins_pos, removed);
            }
        }
        WebsocketOpKind::RevLiveTask { id1, id2 } => {
            let pos1 = live.iter().position(|x| x.id == id1);
            let pos2 = live.iter().position(|x| x.id == id2);

            // order
            let (start_pos, end_pos) = if pos1 <= pos2 {
                (pos1, pos2)
            } else {
                (pos2, pos1)
            };

            // reverse between specified indexes
            if let (Some(start_pos), Some(end_pos)) = (start_pos, end_pos) {
                live.make_contiguous();
                let (s, _) = live.as_mut_slices();
                s[start_pos..=end_pos].reverse();
            }
        }
        WebsocketOpKind::FinishLiveTask { id, status } => {
            if let Some((active_id, _)) = active_id_val {
                if &id == active_id {
                    *active_id_val = None;
                }
            }
            if let Some(pos_in_live) = live.iter().position(|x| x.id == id) {
                finished.push_front(FinishedTask {
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

pub fn report_wmhints_error(e: wm_hints::WmHintsError) -> String {
    log::error!("{}", e);
    e.to_string()
}

pub fn report_url_error(e: url::ParseError) -> String {
    log::error!("{}", e);
    e.to_string()
}
