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

    hotkey_row.connect_apply(|r| {
        let v = r.text().to_string();
        if !v.is_empty() { patch_config(|c| c.general.hotkey = v); }
    });
    lang_row.connect_apply(|r| {
        let v = r.text().to_string();
        if !v.is_empty() { patch_config(|c| c.soniox.language = v); }
    });
    model_row.connect_apply(|r| {
        let v = r.text().to_string();
        if !v.is_empty() { patch_config(|c| c.llm.model = v); }
    });

    let soniox_btn = confirm_button();
    soniox_key_row.add_suffix(&soniox_btn);

    let openrouter_btn = confirm_button();
    openrouter_key_row.add_suffix(&openrouter_btn);

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

    let page = adw::PreferencesPage::new();
    page.add(&general);
    page.add(&speech);
    page.add(&llm_grp);

    let window = adw::PreferencesWindow::new();
    window.set_title(Some("linux-flow Settings"));
    window.set_default_size(480, -1);
    window.add(&page);

    soniox_btn.connect_clicked({
        let row = soniox_key_row.clone();
        let win = window.clone();
        move |_| save_key_and_close("soniox", &row, &win)
    });
    soniox_key_row.connect_apply({
        let win = window.clone();
        move |r| save_key_and_close("soniox", r, &win)
    });
    openrouter_btn.connect_clicked({
        let row = openrouter_key_row.clone();
        let win = window.clone();
        move |_| save_key_and_close("openrouter", &row, &win)
    });
    openrouter_key_row.connect_apply({
        let win = window.clone();
        move |r| save_key_and_close("openrouter", r, &win)
    });

    window
}

fn confirm_button() -> gtk4::Button {
    let btn = gtk4::Button::with_label("Confirm");
    btn.add_css_class("suggested-action");
    btn.set_valign(gtk4::Align::Center);
    btn
}

fn save_key_and_close(
    service: &'static str,
    row: &adw::PasswordEntryRow,
    win: &adw::PreferencesWindow,
) {
    let v = Zeroizing::new(row.text().to_string());
    if !v.is_empty() {
        spawn_save_key(service, v);
        win.close();
    }
}

fn patch_config(update: impl FnOnce(&mut Config)) {
    let mut cfg = Config::load().unwrap_or_default();
    update(&mut cfg);
    let _ = cfg.save();
}

fn spawn_save_key(service: &'static str, value: Zeroizing<String>) {
    std::thread::spawn(move || {
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|rt| rt.block_on(crate::secrets::store_key(service, &value)));
    });
}
