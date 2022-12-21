use iced_winit::winit;
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use xcb_util::ewmh;

pub enum Position {
    Top,
    Bottom,
}

pub struct WmHintsState {
    window_id: u32,
    ewmh_conn: ewmh::Connection,
}

#[derive(Debug)]
pub enum WmHintsError {
    UnsupportedError,
    UnknownError,
}

impl WmHintsState {
    pub fn new(window: &winit::window::Window) -> Result<WmHintsState, WmHintsError> {
        let window_id = match window.raw_window_handle() {
            RawWindowHandle::Xcb(x) => Ok(x.window),
            RawWindowHandle::Xlib(x) => Ok(x.window as u32),
            _ => Err(WmHintsError::UnsupportedError),
        }?;

        let xcb_conn = match window.raw_display_handle() {
            // wrap raw xcb connection
            RawDisplayHandle::Xcb(x) => Ok(unsafe {
                xcb::Connection::from_raw_conn(x.connection as *mut xcb::ffi::xcb_connection_t)
            }),
            // convert display to xcb
            RawDisplayHandle::Xlib(x) => Ok(unsafe {
                xcb::Connection::from_raw_conn(x11::xlib_xcb::XGetXCBConnection(
                    x.display as *mut x11::xlib::Display,
                ) as *mut xcb::ffi::xcb_connection_t)
            }),
            _ => Err(WmHintsError::UnsupportedError),
        }?;

        let ewmh_conn = xcb_util::ewmh::Connection::connect(xcb_conn)
            .map_err(|_| WmHintsError::UnknownError)?;

        return Ok(WmHintsState {
            ewmh_conn,
            window_id,
        });
    }

    // pin to the top of the screen by setting things
    // needs to be run on initialization and whenever the height changes
    pub fn dock_window(&mut self, height: u32, position: Position) {
        ewmh::set_wm_window_type(
            &self.ewmh_conn,
            self.window_id,
            &[self.ewmh_conn.WM_WINDOW_TYPE_DOCK()],
        );

        // TODO: Update _WM_STRUT_PARTIAL if the height/position of the bar changes?
        let mut strut_partial = ewmh::StrutPartial {
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
            left_start_y: 0,
            left_end_y: 0,
            right_start_y: 0,
            right_end_y: 0,
            top_start_x: 0,
            top_end_x: 0,
            bottom_start_x: 0,
            bottom_end_x: 0,
        };
        match position {
            Position::Top => strut_partial.top = height,
            Position::Bottom => strut_partial.bottom = height,
        }
        ewmh::set_wm_strut_partial(&self.ewmh_conn, self.window_id, strut_partial);
    }
}
