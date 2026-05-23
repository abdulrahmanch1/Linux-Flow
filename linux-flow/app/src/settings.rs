use gtk4::prelude::*;
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::config::Config;
use zeroize::Zeroizing;

pub fn build_settings() -> adw::PreferencesWindow {
    let cfg = Config::load().unwrap_or_default();

    let hotkey_row = adw::EntryRow::new();
    hotkey_row.set_title("Push-to-Talk Hotkey");
    hotkey_row.set_text(&cfg.general.hotkey);

    let lang_row = adw::EntryRow::new();
    lang_row.set_title("Language");
    lang_row.set_text(&cfg.soniox.language);

    let model_row = adw::EntryRow::new();
    model_row.set_title("Model");
    model_row.set_text(&cfg.llm.model);

    let soniox_key_row = adw::PasswordEntryRow::new();
    soniox_key_row.set_title("Soniox API Key");

    let openrouter_key_row = adw::PasswordEntryRow::new();
    openrouter_key_row.set_title("OpenRouter API Key");

    let general = adw::PreferencesGroup::new();
    general.set_title("General");
    general.add(&hotkey_row);

    let speech = adw::PreferencesGroup::new();
    speech.set_title("Speech Recognition");
    speech.add(&lang_row);
    speech.add(&soniox_key_row);

    let llm_grp = adw::PreferencesGroup::new();
    llm_grp.set_title("Language Model");
    llm_grp.add(&model_row);
    llm_grp.add(&openrouter_key_row);

    let confirm_btn = gtk4::Button::with_label("Confirm");
    confirm_btn.add_css_class("suggested-action");
    confirm_btn.add_css_class("pill");

    let btn_grp = adw::PreferencesGroup::new();
    btn_grp.add(&confirm_btn);

    let page = adw::PreferencesPage::new();
    page.add(&general);
    page.add(&speech);
    page.add(&llm_grp);
    page.add(&btn_grp);

    let window = adw::PreferencesWindow::new();
    window.set_title(Some("linux-flow Settings"));
    window.set_default_size(480, -1);
    window.add(&page);

    confirm_btn.connect_clicked({
        let win = window.clone();
        move |_| {
            if let Err(e) = patch_config(|c| {
                let v = hotkey_row.text().to_string();
                if !v.is_empty() { c.general.hotkey = v; }
                let v = lang_row.text().to_string();
                if !v.is_empty() { c.soniox.language = v; }
                let v = model_row.text().to_string();
                if !v.is_empty() { c.llm.model = v; }
            }) {
                win.add_toast(adw::Toast::new(&format!("Failed to save settings: {e}")));
                return;
            }
            let soniox_v = Zeroizing::new(soniox_key_row.text().to_string());
            if !soniox_v.is_empty() { spawn_save_key("soniox", soniox_v); }
            let or_v = Zeroizing::new(openrouter_key_row.text().to_string());
            if !or_v.is_empty() { spawn_save_key("openrouter", or_v); }
            win.close();
        }
    });

    window
}

fn patch_config(update: impl FnOnce(&mut Config)) -> Result<(), String> {
    let mut cfg = Config::load().unwrap_or_default();
    update(&mut cfg);
    cfg.save().map_err(|e| e.to_string())
}

fn spawn_save_key(service: &'static str, value: Zeroizing<String>) {
    std::thread::spawn(move || {
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|rt| rt.block_on(crate::secrets::store_key(service, &value)));
    });
}
