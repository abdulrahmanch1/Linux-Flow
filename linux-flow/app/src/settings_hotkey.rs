use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::{config::Config, display::DisplayServer};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn hotkey_row(config: Rc<RefCell<Config>>) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Hotkey");
    row.set_subtitle(&config.borrow().general.hotkey.clone());
    match linux_flow_shared::display::detect() {
        DisplayServer::Wayland => wire_wayland(row.clone(), config),
        _ => wire_x11(row.clone(), config),
    }
    row
}

fn save(config: &Rc<RefCell<Config>>) {
    if let Err(e) = config.borrow().save() {
        tracing::error!("config save: {e}");
    }
}

fn wire_wayland(row: adw::ActionRow, config: Rc<RefCell<Config>>) {
    let btn = gtk4::Button::with_label("Re-register");
    btn.add_css_class("flat");
    btn.set_valign(gtk4::Align::Center);
    btn.connect_clicked(glib::clone!(#[weak] row, #[strong] config, move |btn| {
        btn.set_sensitive(false);
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let r = tokio::runtime::Builder::new_current_thread().enable_all().build()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(crate::wizard_hotkey::bind_portal())
                    .map_err(|e| e.to_string()));
            let _ = tx.send(r);
        });
        let rw = row.downgrade();
        let bw = btn.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(50), glib::clone!(#[strong] config, move || {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    let hotkey = "KEY_RIGHTCTRL".to_string();
                    if let Some(r) = rw.upgrade() { r.set_subtitle(&hotkey); }
                    if let Some(b) = bw.upgrade() { b.set_sensitive(true); }
                    config.borrow_mut().general.hotkey = hotkey;
                    save(&config);
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    tracing::error!("portal rebind: {e}");
                    if let Some(b) = bw.upgrade() { b.set_sensitive(true); }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        }));
    }));
    row.add_suffix(&btn);
    row.set_activatable_widget(Some(&btn));
}

fn wire_x11(row: adw::ActionRow, config: Rc<RefCell<Config>>) {
    let btn = gtk4::Button::with_label("Click, then press a modifier key");
    btn.add_css_class("flat");
    btn.set_focusable(true);
    btn.set_valign(gtk4::Align::Center);
    let key_ctrl = gtk4::EventControllerKey::new();
    let rw = row.downgrade();
    let bw = btn.downgrade();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        let (Some(r), Some(b)) = (rw.upgrade(), bw.upgrade()) else {
            return glib::Propagation::Proceed;
        };
        let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
        if let Some(evdev) = crate::wizard_hotkey::gdk_to_evdev(&name) {
            r.set_subtitle(evdev);
            b.set_label(evdev);
            config.borrow_mut().general.hotkey = evdev.to_string();
            save(&config);
        }
        glib::Propagation::Stop
    });
    btn.add_controller(key_ctrl);
    row.add_suffix(&btn);
    row.set_activatable_widget(Some(&btn));
}
