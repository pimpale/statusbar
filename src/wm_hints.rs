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
    position: Position,
    screen_idx: i32,
    window_id: u32,
    ewmh_conn: ewmh::Connection,
}

#[derive(Debug)]
pub enum WmHintsError {
    UnsupportedError,
    UnknownError,
}

impl WmHintsState {
    pub fn new(
        window: &winit::window::Window,
        position: Position,
    ) -> Result<WmHintsState, WmHintsError> {
        let window_id = match window.raw_window_handle() {
            RawWindowHandle::Xcb(x) => Ok(x.window),
            RawWindowHandle::Xlib(x) => Ok(x.window as u32),
            _ => Err(WmHintsError::UnsupportedError),
        }?;

        let (xcb_conn, screen_idx) = match window.raw_display_handle() {
            // wrap raw xcb connection
            RawDisplayHandle::Xcb(x) => Ok((
                unsafe {
                    xcb::Connection::from_raw_conn(x.connection as *mut xcb::ffi::xcb_connection_t)
                },
                x.screen as i32,
            )),
            // convert display to xcb
            RawDisplayHandle::Xlib(x) => Ok((
                unsafe {
                    xcb::Connection::from_raw_conn(x11::xlib_xcb::XGetXCBConnection(
                        x.display as *mut x11::xlib::Display,
                    )
                        as *mut xcb::ffi::xcb_connection_t)
                },
                x.screen,
            )),
            _ => Err(WmHintsError::UnsupportedError),
        }?;

        let ewmh_conn = xcb_util::ewmh::Connection::connect(xcb_conn)
            .map_err(|_| WmHintsError::UnknownError)?;

        return Ok(WmHintsState {
            position,
            screen_idx,
            ewmh_conn,
            window_id,
        });
    }

    // get screens
    fn screen(&self) -> Result<xcb::Screen<'_>, WmHintsError> {
        self.ewmh_conn
            .get_setup()
            .roots()
            .nth(self.screen_idx as usize)
            .ok_or(WmHintsError::UnknownError)
    }

    // pin to the top of the screen by setting things
    // needs to be run on initialization and whenever the height changes
    pub fn set_ewmh_hints(&mut self, height: u32)  -> Result<(), WmHintsError> {
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

        let screen =self.screen()?;

        match self.position {
            Position::Top => {
                strut_partial.top = height;
                strut_partial.top_start_x = 0;
                strut_partial.top_end_x = screen.width_in_pixels() as u32;
            }
            Position::Bottom => {
                strut_partial.bottom = height;
                strut_partial.bottom_start_x = 0;
                strut_partial.bottom_end_x = screen.width_in_pixels() as u32;
            }
        }
        ewmh::set_wm_strut_partial(&self.ewmh_conn, self.window_id, strut_partial);

        Ok(())
    }

    pub fn update_bar_height(&mut self, height: u32) -> Result<(), WmHintsError> {
        let screen = self.screen()?;
        // If we're at the bottom of the screen, we'll need to update the
        // position of the window.
        let y = match self.position {
            Position::Top => 0,
            Position::Bottom => screen.height_in_pixels() - height as u16,
        };

        // Update the height/position of the XCB window and the height of the Cairo surface.
        let values = [
            (xcb::CONFIG_WINDOW_X as u16, 0),
            (xcb::CONFIG_WINDOW_Y as u16, u32::from(y)),
            (xcb::CONFIG_WINDOW_WIDTH as u16, u32::from(screen.width_in_pixels())),
            (xcb::CONFIG_WINDOW_HEIGHT as u16, u32::from(height)),
            (xcb::CONFIG_WINDOW_STACK_MODE as u16, xcb::STACK_MODE_ABOVE),
        ];
        xcb::configure_window(&self.ewmh_conn, self.window_id, &values);
        xcb::map_window(&self.ewmh_conn, self.window_id);

        // Update EWMH properties - we might need to reserve more or less space.
        self.set_ewmh_hints(height)?;

        Ok(())
    }
}
