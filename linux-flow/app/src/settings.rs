use gtk4::prelude::*;
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::config::Config;
use std::{cell::Cell, rc::Rc, sync::mpsc};
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
    let split_row = adw::SwitchRow::new();
    split_row.set_title("Split into multiple messages");
    split_row.set_subtitle("Let dictation break into chat-style lines. Off keeps one line (safer in terminals).");
    split_row.set_active(cfg.llm.message_splitting);
    let soniox_key_row = adw::PasswordEntryRow::new();
    soniox_key_row.set_title("Soniox API Key");
    let openrouter_key_row = adw::PasswordEntryRow::new();
    openrouter_key_row.set_title("OpenRouter API Key");

    let general = adw::PreferencesGroup::new();
    general.set_title("General");
    general.add(&hotkey_row);

    let dictation = adw::PreferencesGroup::new();
    dictation.set_title("Dictation");
    dictation.add(&split_row);

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
    page.add(&dictation);
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
                if !v.is_empty() {
                    c.general.hotkey = v;
                }
                let v = lang_row.text().to_string();
                if !v.is_empty() {
                    c.soniox.language = v;
                }
                let v = model_row.text().to_string();
                if !v.is_empty() {
                    c.llm.model = v;
                }
                c.llm.message_splitting = split_row.is_active();
            }) {
                win.add_toast(adw::Toast::new(&format!("Failed to save settings: {e}")));
                return;
            }
            let (tx, rx) = mpsc::channel::<(&'static str, Result<(), String>)>();
            let mut expected: u32 = 0;

            let soniox_v = Zeroizing::new(soniox_key_row.text().to_string());
            if !soniox_v.is_empty() {
                expected += 1;
                spawn_save_key("soniox", "Soniox", soniox_v, tx.clone());
            }
            let or_v = Zeroizing::new(openrouter_key_row.text().to_string());
            if !or_v.is_empty() {
                expected += 1;
                spawn_save_key("openrouter", "OpenRouter", or_v, tx.clone());
            }
            drop(tx);
            if expected == 0 { win.close(); return; }

            let remaining = Rc::new(Cell::new(expected));
            let win_c = win.clone();
            gtk4::glib::timeout_add_local(
                std::time::Duration::from_millis(50),
                move || {
                    loop {
                        match rx.try_recv() {
                            Ok((name, result)) => {
                                remaining.set(remaining.get() - 1);
                                if let Err(e) = result {
                                    win_c.add_toast(adw::Toast::new(
                                        &format!("Failed to save {name} key: {e}"),
                                    ));
                                }
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => { remaining.set(0); break; }
                        }
                    }
                    if remaining.get() == 0 {
                        win_c.close();
                        return gtk4::glib::ControlFlow::Break;
                    }
                    gtk4::glib::ControlFlow::Continue
                },
            );
        }
    });

    window
}

fn patch_config(update: impl FnOnce(&mut Config)) -> Result<(), String> {
    let mut cfg = Config::load().map_err(|e| e.to_string())?;
    update(&mut cfg);
    cfg.save().map_err(|e| e.to_string())
}

fn spawn_save_key(
    service: &'static str,
    label: &'static str,
    value: Zeroizing<String>,
    tx: mpsc::Sender<(&'static str, Result<(), String>)>,
) {
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
            .and_then(|rt| {
                rt.block_on(crate::secrets::store_key(service, &value))
                    .map_err(|e| e.to_string())
            });
        let _ = tx.send((label, result));
    });
}
