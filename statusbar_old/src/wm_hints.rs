use derivative::Derivative;
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, XcbDisplayHandle,
    XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};
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


#[derive(Debug, Clone, Copy)]
pub enum WindowType {
    Dock,
    Toolbar,
    Menu,
    Utility,
    Splash,
    Dialog,
    DropdownMenu,
    PopupMenu,
    Tooltip,
    Notification,
    Combo,
    Dnd,
    Normal,
}

pub fn create_state_mgr<T>(window: &T) -> Result<WmHintsState, WmHintsError>
where
    T: HasWindowHandle + HasDisplayHandle,
{
    let raw_window_handle = window.window_handle().unwrap().as_raw();
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

impl WmHintsState {
    pub fn grab_keyboard(&self) -> Result<(), WmHintsError> {
        let cookie = self.conn.send_request(&x::GrabKeyboard {
            owner_events: false,
            grab_window: self.window,
            time: x::CURRENT_TIME,
            keyboard_mode: x::GrabMode::Async,
            pointer_mode: x::GrabMode::Async,
        });
        let reply = self
            .conn
            .wait_for_reply(cookie)
            .map_err(|x| WmHintsError::XcbError(x))?;

        // return based on reply status
        match reply.status() {
            x::GrabStatus::Success => Ok(()),
            e => Err(WmHintsError::XcbGrabStatusError(e)),
        }
    }

    pub fn ungrab_keyboard(&self) -> Result<(), WmHintsError> {
        self.conn
            .send_and_check_request(&x::UngrabKeyboard {
                time: x::CURRENT_TIME,
            })
            .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;
        Ok(())
    }

    pub fn map_window(&self) -> Result<(), WmHintsError> {
        self.conn
            .send_and_check_request(&x::MapWindow {
                window: self.window,
            })
            .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;
        Ok(())
    }

    pub fn unmap_window(&self) -> Result<(), WmHintsError> {
        self.conn
            .send_and_check_request(&x::UnmapWindow {
                window: self.window,
            })
            .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;
        Ok(())
    }

    fn atom_name(&self, atom: xcb::x::Atom) -> String {
        let cookie = self.conn.send_request(&x::GetAtomName { atom });
        let reply = self.conn.wait_for_reply(cookie).unwrap();
        reply.name().to_string()
    }

    fn set_window_type(&self, window_type: WindowType) -> Result<(), WmHintsError> {
        // Get the _NET_WM_WINDOW_TYPE atom
        let wm_type_cookie = self.conn.send_request(&x::InternAtom {
            only_if_exists: true,
            name: "_NET_WM_WINDOW_TYPE".as_bytes(),
        });
        let wm_type_reply = self.conn
            .wait_for_reply(wm_type_cookie)
            .map_err(|x| WmHintsError::XcbError(x))?;
        
        // Get the specific window type atom (e.g. _NET_WM_WINDOW_TYPE_DOCK)
        let type_name = match window_type {
            WindowType::Dock => "_NET_WM_WINDOW_TYPE_DOCK",
            WindowType::Toolbar => "_NET_WM_WINDOW_TYPE_TOOLBAR",
            WindowType::Menu => "_NET_WM_WINDOW_TYPE_MENU",
            WindowType::Utility => "_NET_WM_WINDOW_TYPE_UTILITY",
            WindowType::Splash => "_NET_WM_WINDOW_TYPE_SPLASH",
            WindowType::Dialog => "_NET_WM_WINDOW_TYPE_DIALOG",
            WindowType::DropdownMenu => "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
            WindowType::PopupMenu => "_NET_WM_WINDOW_TYPE_POPUP_MENU",
            WindowType::Tooltip => "_NET_WM_WINDOW_TYPE_TOOLTIP",
            WindowType::Notification => "_NET_WM_WINDOW_TYPE_NOTIFICATION",
            WindowType::Combo => "_NET_WM_WINDOW_TYPE_COMBO",
            WindowType::Dnd => "_NET_WM_WINDOW_TYPE_DND",
            WindowType::Normal => "_NET_WM_WINDOW_TYPE_NORMAL",
        };
        
        let type_cookie = self.conn.send_request(&x::InternAtom {
            only_if_exists: true,
            name: type_name.as_bytes(),
        });
        let type_reply = self.conn
            .wait_for_reply(type_cookie)
            .map_err(|x| WmHintsError::XcbError(x))?;

        // Set the window type property
        self.conn
            .send_and_check_request(&x::ChangeProperty {
                mode: x::PropMode::Replace,
                window: self.window,
                property: wm_type_reply.atom(),
                r#type: x::ATOM_ATOM,
                data: &[type_reply.atom()],
            })
            .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;

        Ok(())
    }

    fn configure_window(&self, height: u32) -> Result<(), WmHintsError> {
        use x::ConfigWindow;
        let values = [ConfigWindow::Height(height)];

        self.conn
            .send_and_check_request(&x::ConfigureWindow {
                window: self.window,
                value_list: &values,
            })
            .map_err(|x| WmHintsError::XcbError(xcb::Error::Protocol(x)))?;
        
        Ok(())
    }

    pub fn dock_window(&self, height: u32) -> Result<(), WmHintsError> {
        // First unmap the window
        self.unmap_window()?;

        // Configure the window height
        self.configure_window(height)?;

        // Set it as a dock window
        self.set_window_type(WindowType::Dock)?;

        // Finally remap the window
        self.map_window()?;

        Ok(())
    }
}

