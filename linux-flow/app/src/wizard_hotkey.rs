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
