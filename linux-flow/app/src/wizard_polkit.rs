use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;
use linux_flow_shared::config::Config;
use zeroize::{Zeroize, Zeroizing};

use crate::wizard::WizardState;

const HELPER_PATH: &str = "/usr/local/lib/linux-flow/linux-flow-helper";
const POLKIT_ACTION: &str = "org.freedesktop.policykit.exec";

pub(crate) fn page_polkit(
    state: Rc<RefCell<WizardState>>,
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

    let finish = gtk4::Button::with_label("Finish");
    finish.set_sensitive(false);
    finish.add_css_class("suggested-action");

    wire_auth(auth_btn.clone(), status.clone(), finish.clone());
    wire_finish(Rc::clone(&state), status.clone(), finish.clone());

    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    btn_row.append(&auth_btn);
    btn_row.append(&finish);

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

fn wire_auth(auth_btn: gtk4::Button, status: gtk4::Label, next: gtk4::Button) {
    auth_btn.connect_clicked(glib::clone!(
        #[weak] status, #[weak] next, #[weak] auth_btn,
        move |_| {
            auth_btn.set_sensitive(false);
            status.set_text("Requesting authorization…");
            let pid = std::process::id().to_string();
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let r = std::process::Command::new("pkcheck")
                    .args(["--action-id", POLKIT_ACTION, "--process", &pid,
                           "--detail", "program", HELPER_PATH, "--allow-user-interaction"])
                    .status()
                    .map_err(|e| e.to_string())
                    .and_then(|s| if s.success() { Ok(()) } else {
                        Err(format!("authorization denied (exit {})",
                            s.code().map_or_else(|| "signal".to_string(), |c| c.to_string())))
                    });
                let _ = tx.send(r);
            });
            let sw = status.downgrade(); let nw = next.downgrade(); let aw = auth_btn.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let (Some(s), Some(n), Some(a)) = (sw.upgrade(), nw.upgrade(), aw.upgrade())
                    else { return glib::ControlFlow::Break; };
                match rx.try_recv() {
                    Ok(Ok(())) => { s.set_text("Authorization granted."); n.set_sensitive(true); glib::ControlFlow::Break }
                    Ok(Err(e)) => { s.set_text(&e); a.set_sensitive(true); glib::ControlFlow::Break }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                }
            });
        }
    ));
}

fn wire_finish(state: Rc<RefCell<WizardState>>, status: gtk4::Label, finish: gtk4::Button) {
    finish.connect_clicked(glib::clone!(
        #[weak] status, #[strong] state,
        move |btn| {
            btn.set_sensitive(false);
            status.set_text("Saving credentials…");
            let mut soniox = Zeroizing::new(state.borrow().soniox_key.as_str().to_owned());
            let mut openrouter = Zeroizing::new(state.borrow().openrouter_key.as_str().to_owned());
            state.borrow_mut().soniox_key.zeroize();
            state.borrow_mut().openrouter_key.zeroize();
            let model = state.borrow().model.clone();
            let hotkey = state.borrow().hotkey.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let r = tokio::runtime::Builder::new_current_thread().enable_all().build()
                    .map_err(|e| e.to_string())
                    .and_then(|rt| rt.block_on(async {
                        crate::secrets::store_key("soniox", &soniox).await.map_err(|e| e.to_string())?;
                        soniox.zeroize();
                        crate::secrets::store_key("openrouter", &openrouter).await.map_err(|e| e.to_string())?;
                        openrouter.zeroize();
                        let mut cfg = Config::default();
                        cfg.llm.model = model; cfg.general.hotkey = hotkey;
                        cfg.save().map_err(|e| e.to_string())
                    }));
                let _ = tx.send(r);
            });
            let sw = status.downgrade(); let bw = btn.downgrade();
            let state2 = Rc::clone(&state);
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let (Some(s), Some(b)) = (sw.upgrade(), bw.upgrade())
                    else { return glib::ControlFlow::Break; };
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        if let Some(root) = s.root() {
                            if let Ok(w) = root.downcast::<gtk4::Window>() { w.close(); }
                        }
                        (state2.borrow().on_complete)();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => { s.set_text(&format!("Error: {e}")); b.set_sensitive(true); glib::ControlFlow::Break }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                }
            });
        }
    ));
}
