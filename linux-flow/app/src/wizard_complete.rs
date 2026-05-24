use std::{cell::RefCell, rc::Rc};

use gtk4::{glib, prelude::*};
use linux_flow_shared::config::Config;
use zeroize::Zeroizing;

use crate::wizard::WizardState;

pub(crate) fn spawn_finish(
    state: Rc<RefCell<WizardState>>,
    next: gtk4::Button,
    status: gtk4::Label,
) {
    next.set_sensitive(false);
    status.set_text("Saving configuration…");

    let soniox_key: Zeroizing<String>;
    let openrouter_key: Zeroizing<String>;
    let model: String;
    let hotkey: String;
    {
        let st = state.borrow();
        soniox_key = st.soniox_key.clone();
        openrouter_key = st.openrouter_key.clone();
        model = st.model.clone();
        hotkey = st.hotkey.clone();
    }

    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
            .and_then(|rt| {
                rt.block_on(async {
                    crate::secrets::store_key("soniox", &soniox_key)
                        .await
                        .map_err(|e| e.to_string())?;
                    crate::secrets::store_key("openrouter", &openrouter_key)
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut cfg = Config::load().map_err(|e| e.to_string())?;
                    cfg.llm.model = model;
                    cfg.general.hotkey = hotkey;
                    cfg.save().map_err(|e| e.to_string())
                })
            });
        // soniox_key and openrouter_key are Zeroized on drop here
        let _ = tx.send(result);
    });

    let next_w = next.downgrade();
    let status_w = status.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let (Some(n), Some(s)) = (next_w.upgrade(), status_w.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                if let Some(root) = n.root() {
                    if let Ok(window) = root.downcast::<gtk4::Window>() {
                        window.close();
                    }
                }
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                s.set_text(&format!("Error saving: {e}"));
                n.set_sensitive(true);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        }
    });
}
