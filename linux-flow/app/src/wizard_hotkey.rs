use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;
use linux_flow_shared::display::DisplayServer;

use crate::wizard::WizardState;

pub(crate) fn page_hotkey(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    match linux_flow_shared::display::detect() {
        DisplayServer::Wayland => page_wayland(state, nav),
        _ => page_x11(state, nav),
    }
}

fn gdk_to_evdev(name: &str) -> Option<&'static str> {
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

async fn bind_portal() -> Result<(), String> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    let shortcuts = GlobalShortcuts::new().await.map_err(|e| e.to_string())?;
    let session = shortcuts.create_session().await.map_err(|e| e.to_string())?;
    shortcuts
        .bind_shortcuts(
            &session,
            &[NewShortcut::new("record", "Push to Talk — linux-flow")
                .preferred_trigger("Control_R")],
            None,
        )
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn page_wayland(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    let status = gtk4::Label::new(Some(
        "Click to register the Right Ctrl push-to-talk hotkey via the system portal.",
    ));
    status.set_wrap(true);

    let grant = gtk4::Button::with_label("Grant Hotkey Access");
    grant.add_css_class("suggested-action");

    // X11 fallback widget — hidden until portal fails.
    let capture = gtk4::Button::with_label("Right Ctrl  (click then press a key to change)");
    capture.set_focusable(true);
    capture.set_visible(false);

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    let key_ctrl = gtk4::EventControllerKey::new();
    {
        let capture = capture.clone();
        let status = status.clone();
        let state = Rc::clone(&state);
        key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
            let gdk_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
            match gdk_to_evdev(&gdk_name) {
                Some(evdev) => {
                    state.borrow_mut().hotkey = evdev.to_string();
                    capture.set_label(&format!("{gdk_name}  (press to change)"));
                    status.set_text(&format!("{gdk_name} set as push-to-talk key."));
                }
                None => status.set_text("Use a modifier key (Ctrl, Alt, Super, Shift, Caps Lock)."),
            }
            glib::Propagation::Stop
        });
    }
    capture.add_controller(key_ctrl);

    next.connect_clicked(glib::clone!(#[weak] nav, #[strong] state, move |_| {
        nav.push(&crate::wizard_polkit::page_polkit(Rc::clone(&state), nav.clone()));
    }));

    grant.connect_clicked(glib::clone!(
        #[weak] status, #[weak] next, #[weak] grant, #[weak] capture,
        move |_| {
            grant.set_sensitive(false);
            status.set_text("Contacting portal…");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())
                    .and_then(|rt| rt.block_on(bind_portal()));
                let _ = tx.send(result);
            });

            let status_w = status.downgrade();
            let next_w = next.downgrade();
            let grant_w = grant.downgrade();
            let capture_w = capture.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let (Some(s), Some(n), Some(g), Some(c)) =
                    (status_w.upgrade(), next_w.upgrade(), grant_w.upgrade(), capture_w.upgrade())
                else {
                    return glib::ControlFlow::Break;
                };
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        s.set_text("Hotkey registered successfully.");
                        n.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(_)) => {
                        s.set_text(
                            "Portal unavailable — press a modifier key below (default: Right Ctrl).",
                        );
                        g.set_visible(false);
                        c.set_visible(true);
                        n.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                }
            });
        }
    ));

    let next_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    next_row.set_halign(gtk4::Align::End);
    next_row.append(&next);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(&status);
    body.append(&grant);
    body.append(&capture);
    body.append(&next_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, "Hotkey")
}

fn page_x11(state: Rc<RefCell<WizardState>>, nav: adw::NavigationView) -> adw::NavigationPage {
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
