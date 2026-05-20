use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;

use crate::wizard::WizardState;
use crate::wizard_hotkey::gdk_to_evdev;

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
        match gdk_to_evdev(&gdk_name) {
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

    build_page(&status, &capture, next, "Hotkey")
}

fn build_page(
    top: &impl IsA<gtk4::Widget>,
    middle: &impl IsA<gtk4::Widget>,
    next: gtk4::Button,
    title: &str,
) -> adw::NavigationPage {
    let next_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    next_row.set_halign(gtk4::Align::End);
    next_row.append(&next);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(top.upcast_ref::<gtk4::Widget>());
    body.append(middle.upcast_ref::<gtk4::Widget>());
    body.append(&next_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, title)
}
