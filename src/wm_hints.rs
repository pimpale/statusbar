use derivative::Derivative;
use iced_winit::winit;
use xcb::{x, XidNew};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WmHintsState {
    screen_id: i32,
    #[derivative(Debug = "ignore")]
    conn: xcb::Connection,
    window: xcb::x::Window,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum WmHintsError {
    UnsupportedError,
    XcbError(xcb::Error),
    XcbGrabStatusError(xcb::x::GrabStatus),
}

impl std::fmt::Display for WmHintsError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::UnsupportedError => write!(f, "Platform does not support X11"),
            Self::XcbError(e) => write!(f, "XCB: {}", e),
            Self::XcbGrabStatusError(e) => write!(
                f,
                "Couldn't Grab: {}",
                match e {
                    x::GrabStatus::AlreadyGrabbed => "already grabbed",
                    x::GrabStatus::InvalidTime => "invalid time",
                    x::GrabStatus::NotViewable => "not viewable",
                    x::GrabStatus::Frozen => "frozen",
                    _ => "unknown",
                }
            ),
        }
    }
}

impl std::error::Error for WmHintsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::XcbError(e) => Some(e),
            _ => None,
        }
    }
}

pub fn create_state_mgr(
    window: &dyn winit::platform::unix::WindowExtUnix,
) -> Result<WmHintsState, WmHintsError> {
    Ok(WmHintsState {
        screen_id: window
            .xlib_screen_id()
            .ok_or(WmHintsError::UnsupportedError)? as i32,
        conn: unsafe {
            xcb::Connection::from_raw_conn(
                window
                    .xcb_connection()
                    .ok_or(WmHintsError::UnsupportedError)?
                    as *mut xcb::ffi::xcb_connection_t,
            )
        },
        window: unsafe {
            x::Window::new(window.xlib_window().ok_or(WmHintsError::UnsupportedError)? as u32)
        },
    })
}

pub fn grab_keyboard(data: &WmHintsState) -> Result<(), WmHintsError> {
    let cookie = data.conn.send_request(&x::GrabKeyboard {
        owner_events: false,
        grab_window: data.window,
        time: x::CURRENT_TIME,
        keyboard_mode: x::GrabMode::Async,
        pointer_mode: x::GrabMode::Async,
    });
    let reply = data
        .conn
        .wait_for_reply(cookie)
        .map_err(|x| WmHintsError::XcbError(x))?;

    // return based on reply status
    match reply.status() {
        x::GrabStatus::Success => Ok(()),
        e => Err(WmHintsError::XcbGrabStatusError(e)),
    }
}

pub fn ungrab_keyboard(data: &WmHintsState) -> Result<(), WmHintsError> {
    data.conn
        .send_and_check_request(&x::UngrabKeyboard {
            time: x::CURRENT_TIME,
        })
        .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;
    Ok(())
}
