use derivative::Derivative;
use iced_wgpu::wgpu::rwh::{
    HasDisplayHandle, RawDisplayHandle, RawWindowHandle, XcbDisplayHandle, XcbWindowHandle,
    XlibDisplayHandle, XlibWindowHandle,
};
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

pub fn create_state_mgr(window: &winit::window::Window) -> Result<WmHintsState, WmHintsError> {
    let raw_window_handle = winit::raw_window_handle::HasWindowHandle::window_handle(&window)
        .unwrap()
        .as_raw();
    let raw_display_handle = window.display_handle().unwrap().as_raw();

    let (screen_id, conn) = match raw_display_handle {
        RawDisplayHandle::Xcb(XcbDisplayHandle {
            screen,
            connection: Some(connection),
            ..
        }) => (
            screen,
            connection.as_ptr() as *mut xcb::ffi::xcb_connection_t,
        ),
        RawDisplayHandle::Xlib(XlibDisplayHandle {
            screen,
            display: Some(display),
            ..
        }) => unsafe {
            (
                screen,
                xcb::ffi::XGetXCBConnection(display.as_ptr() as *mut x11::xlib::_XDisplay),
            )
        },
        _ => return Err(WmHintsError::UnsupportedError),
    };

    let window_id = match raw_window_handle {
        RawWindowHandle::Xcb(XcbWindowHandle { window, .. }) => u32::from(window),
        RawWindowHandle::Xlib(XlibWindowHandle { window, .. }) => window as u32,
        _ => return Err(WmHintsError::UnsupportedError),
    };

    Ok(WmHintsState {
        screen_id,
        conn: unsafe { xcb::Connection::from_raw_conn(conn) },
        window: unsafe { x::Window::new(window_id) },
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
