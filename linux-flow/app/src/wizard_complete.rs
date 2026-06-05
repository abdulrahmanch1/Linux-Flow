use std::{cell::RefCell, rc::Rc};

use gtk4::prelude::*;
use linux_flow_shared::config::Config;
use zeroize::Zeroizing;

use crate::wizard::WizardState;

/// Persists a single verified key to the keyring the moment the user advances
/// past its page, so a wizard closed before "Finish" doesn't lose it — a resume
/// then picks up only the pieces still missing. On success runs `on_stored`
/// (which advances to the next page); on failure surfaces the error and
/// re-enables the button to retry. Mirrors the spawn-thread + current-thread
/// runtime + `glib::timeout_add_local` poll pattern used elsewhere here.
pub(crate) fn spawn_store_key(
    service: &'static str,
    key: Zeroizing<String>,
    status: &gtk4::Label,
    next: &gtk4::Button,
    on_stored: impl Fn() + 'static,
) {
    next.set_sensitive(false);
    let status_w = status.downgrade();
    let next_w = next.downgrade();
    crate::gtk_async::run_worker(
        move || {
            // key is zeroized on drop when this closure returns.
            crate::gtk_async::block_on_new_rt(
                async move {
                    crate::secrets::store_key(service, &key)
                        .await
                        .map_err(|e| e.to_string())
                },
                Err,
            )
        },
        move |result: Result<(), String>| {
            let (Some(s), Some(n)) = (status_w.upgrade(), next_w.upgrade()) else {
                return;
            };
            match result {
                Ok(()) => on_stored(),
                Err(e) => {
                    s.set_text(&format!("Couldn't save the key: {e}"));
                    n.set_sensitive(true);
                }
            }
        },
    );
}

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
    let needs;
    {
        let st = state.borrow();
        soniox_key = st.soniox_key.clone();
        openrouter_key = st.openrouter_key.clone();
        model = st.model.clone();
        hotkey = st.hotkey.clone();
        needs = st.needs;
    }

    let next_w = next.downgrade();
    let status_w = status.downgrade();
    crate::gtk_async::run_worker(
        move || {
            // soniox_key and openrouter_key are zeroized on drop when this returns.
            crate::gtk_async::block_on_new_rt(
                async move {
                    // Only write a key the user actually re-entered; skipping a
                    // present key avoids clobbering it with an empty value.
                    if needs.soniox_key {
                        crate::secrets::store_key("soniox", &soniox_key)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    if needs.openrouter_key {
                        crate::secrets::store_key("openrouter", &openrouter_key)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    let mut cfg = Config::load().map_err(|e| e.to_string())?;
                    cfg.llm.model = model;
                    cfg.general.hotkey = hotkey;
                    cfg.save().map_err(|e| e.to_string())
                },
                Err,
            )
        },
        move |result: Result<(), String>| {
            let (Some(n), Some(s)) = (next_w.upgrade(), status_w.upgrade()) else {
                return;
            };
            match result {
                Ok(()) => {
                    if let Some(root) = n.root() {
                        if let Ok(window) = root.downcast::<gtk4::Window>() {
                            window.close();
                        }
                    }
                }
                Err(e) => {
                    s.set_text(&format!("Error saving: {e}"));
                    n.set_sensitive(true);
                }
            }
        },
    );
}
