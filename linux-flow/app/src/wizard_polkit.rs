use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;

use crate::wizard::WizardState;

const HELPER_PATH: &str = "/usr/local/lib/linux-flow/linux-flow-helper";
const POLKIT_ACTION: &str = "org.freedesktop.policykit.exec";

pub(crate) fn page_polkit(
    _state: Rc<RefCell<WizardState>>,
    _nav: adw::NavigationView,
) -> adw::NavigationPage {
    let desc = gtk4::Label::new(Some(
        "linux-flow uses a privileged helper to inject keystrokes via /dev/uinput. \
         Click Authorize to grant the required permission.",
    ));
    desc.set_wrap(true);

    let status = gtk4::Label::new(None);
    status.set_wrap(true);

    let auth_btn = gtk4::Button::with_label("Authorize");
    auth_btn.add_css_class("suggested-action");

    let next = gtk4::Button::with_label("Finish");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    next.connect_clicked(move |_| { /* wizard_complete will be called here */ });

    auth_btn.connect_clicked(
        glib::clone!(#[weak] status, #[weak] next, #[weak] auth_btn, move |_| {
            auth_btn.set_sensitive(false);
            status.set_text("Requesting authorization…");

            let pid = std::process::id().to_string();
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let result = std::process::Command::new("pkcheck")
                    .args([
                        "--action-id", POLKIT_ACTION,
                        "--process", &pid,
                        "--detail", "program", HELPER_PATH,
                        "--allow-user-interaction",
                    ])
                    .status()
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        if s.success() {
                            Ok(())
                        } else {
                            let code = s.code().map_or_else(|| "signal".to_string(), |c| c.to_string());
                            Err(format!("authorization denied (exit {code})"))
                        }
                    });
                let _ = tx.send(result);
            });

            let status_w = status.downgrade();
            let next_w = next.downgrade();
            let auth_btn_w = auth_btn.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let (Some(s), Some(n), Some(a)) =
                    (status_w.upgrade(), next_w.upgrade(), auth_btn_w.upgrade())
                else {
                    return glib::ControlFlow::Break;
                };
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        s.set_text("Authorization granted.");
                        n.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        s.set_text(&e.to_string());
                        a.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                }
            });
        }),
    );

    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    btn_row.append(&auth_btn);
    btn_row.append(&next);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(&desc);
    body.append(&status);
    body.append(&btn_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, "Authorize Helper")
}
