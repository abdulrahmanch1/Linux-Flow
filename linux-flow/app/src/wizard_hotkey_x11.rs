use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;

use crate::wizard::WizardState;

pub(crate) fn page_x11(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    let status = gtk4::Label::new(Some(
        "Click the button and press a modifier key to set your push-to-talk hotkey.",
    ));
    status.set_wrap(true);

    let capture = gtk4::Button::with_label("Right Ctrl (click, then press a key)");
    capture.set_focusable(true);

    let next = gtk4::Button::with_label("Next");
    next.add_css_class("suggested-action");

    next.connect_clicked(glib::clone!(#[weak] nav, #[strong] state, move |_| {
        nav.push(&crate::wizard_polkit::page_polkit(Rc::clone(&state), nav.clone()));
    }));

    let key_ctrl = gtk4::EventControllerKey::new();
    let capture_w = capture.downgrade();
    let status_w = status.downgrade();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        let (Some(cap), Some(sts)) = (capture_w.upgrade(), status_w.upgrade()) else {
            return glib::Propagation::Proceed;
        };
        let gdk_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
        match crate::wizard_hotkey::gdk_to_evdev(&gdk_name) {
            Some(evdev) => {
                state.borrow_mut().hotkey = evdev.to_string();
                cap.set_label(&format!("{gdk_name} — press to change"));
                sts.set_text(&format!("{gdk_name} set as push-to-talk key."));
            }
            None => sts.set_text(
                "Unsupported key. Use a modifier key (Ctrl, Alt, Super, Shift, Caps Lock).",
            ),
        }
        glib::Propagation::Stop
    });
    capture.add_controller(key_ctrl);

    crate::wizard_hotkey::build_page(&status, &capture, next, "Hotkey")
}
