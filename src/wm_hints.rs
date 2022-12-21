use iced_winit::winit;
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use xcb_util::ewmh;

pub struct WmHintsState {
    emwh_conn: ewmh::Connection,
}

pub enum WmHintsError {
    UnsupportedError,
    UnknownError,
}

impl WmHintsState {
    pub fn new(window: winit::window::Window) -> Result<WmHintsState, WmHintsError> {
        let xcb_window_handle = match window.raw_window_handle() {
            RawWindowHandle::Xcb(x) => Ok(x),
            _ => Err(WmHintsError::UnsupportedError),
        }?;
        let xcb_display_handle = match window.raw_display_handle() {
            RawDisplayHandle::Xcb(x) => x,
            _ => unimplemented!(),
        };

        let xcb_conn = unsafe {
            xcb::Connection::from_raw_conn(
                xcb_display_handle.connection as *mut xcb::ffi::xcb_connection_t,
            )
        };

        let emwh_conn = xcb_util::ewmh::Connection::connect(xcb_conn).map_err(|_| UnknownError);
        return Ok(WmHintsState { emwh_conn });
    }

    // pin to the top of the screen by setting things
    // needs to be run on initialization and whenever the height changes
    pub fn pin_top(&mut self) {
    }
}
