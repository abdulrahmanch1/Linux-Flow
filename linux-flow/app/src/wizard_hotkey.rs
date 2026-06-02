use std::{cell::RefCell, rc::Rc};

use libadwaita as adw;
use linux_flow_shared::display::DisplayServer;

use crate::wizard::WizardState;

pub(crate) fn gdk_to_evdev(name: &str) -> Option<&'static str> {
    match name {
        "Control_R" => Some("KEY_RIGHTCTRL"),
        "Control_L" => Some("KEY_LEFTCTRL"),
        "Alt_R" => Some("KEY_RIGHTALT"),
        "Alt_L" => Some("KEY_LEFTALT"),
        "Super_L" => Some("KEY_LEFTMETA"),
        "Super_R" => Some("KEY_RIGHTMETA"),
        "Shift_R" => Some("KEY_RIGHTSHIFT"),
        "Shift_L" => Some("KEY_LEFTSHIFT"),
        "Caps_Lock" => Some("KEY_CAPSLOCK"),
        "F13" => Some("KEY_F13"),
        "F14" => Some("KEY_F14"),
        "F15" => Some("KEY_F15"),
        "F16" => Some("KEY_F16"),
        "F17" => Some("KEY_F17"),
        "F18" => Some("KEY_F18"),
        "F19" => Some("KEY_F19"),
        "F20" => Some("KEY_F20"),
        "F21" => Some("KEY_F21"),
        "F22" => Some("KEY_F22"),
        "F23" => Some("KEY_F23"),
        "F24" => Some("KEY_F24"),
        _ => None,
    }
}

pub(crate) fn page_hotkey(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    match linux_flow_shared::display::detect() {
        DisplayServer::Wayland => crate::wizard_hotkey_wayland::page_wayland(state, nav),
        _ => crate::wizard_hotkey_x11::page_x11(state, nav),
    }
}
