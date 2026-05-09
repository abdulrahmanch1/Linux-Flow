use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::config::Config;
use std::{cell::RefCell, rc::Rc};
use zeroize::Zeroizing;

/// Build the settings window pre-populated with `config`. Every change is
/// written to disk (config.toml) or the keyring immediately on apply.
pub fn build_settings(config: Config) -> adw::PreferencesWindow {
    let config = Rc::new(RefCell::new(config));

    let voice_group = adw::PreferencesGroup::new();
    voice_group.set_title("Voice Input");
    voice_group.add(&crate::settings_hotkey::hotkey_row(Rc::clone(&config)));
    voice_group.add(&text_row(
        "Soniox Language",
        &config.borrow().soniox.language.clone(),
        { let c = Rc::clone(&config); move |v| { c.borrow_mut().soniox.language = v; persist(&c); } },
    ));

    let model_group = adw::PreferencesGroup::new();
    model_group.set_title("AI Model");
    model_group.add(&text_row(
        "OpenRouter Model",
        &config.borrow().llm.model.clone(),
        { let c = Rc::clone(&config); move |v| { c.borrow_mut().llm.model = v; persist(&c); } },
    ));

    let keys_group = adw::PreferencesGroup::new();
    keys_group.set_title("API Keys");
    keys_group.add(&key_row("Soniox API Key", "soniox"));
    keys_group.add(&key_row("OpenRouter API Key", "openrouter"));

    let page = adw::PreferencesPage::new();
    page.add(&voice_group);
    page.add(&model_group);
    page.add(&keys_group);

    let win = adw::PreferencesWindow::new();
    win.set_title(Some("linux-flow Settings"));
    win.set_default_size(480, 520);
    win.add(&page);
    win
}

fn persist(config: &Rc<RefCell<Config>>) {
    if let Err(e) = config.borrow().save() {
        tracing::error!("config save: {e}");
    }
}

fn text_row(title: &str, value: &str, on_apply: impl Fn(String) + 'static) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_text(value);
    row.set_show_apply_button(true);
    row.connect_apply(move |r| {
        let t = r.text().to_string();
        if !t.is_empty() { on_apply(t); }
    });
    row
}

fn key_row(title: &str, service: &'static str) -> adw::PasswordEntryRow {
    let row = adw::PasswordEntryRow::new();
    row.set_title(title);
    row.set_show_apply_button(true);
    row.connect_apply(glib::clone!(#[weak] row, move |_| {
        let value = row.text().to_string();
        if value.is_empty() { return; }
        row.set_text("");
        std::thread::spawn(move || {
            let key = Zeroizing::new(value);
            if let Err(e) = tokio::runtime::Builder::new_current_thread()
                .enable_all().build()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(crate::secrets::store_key(service, &key))
                    .map_err(|e| e.to_string()))
            {
                tracing::error!("key store ({service}): {e}");
            }
        });
    }));
    row
}
