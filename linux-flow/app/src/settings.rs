use gtk4::prelude::*;
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::config::Config;
use std::{cell::Cell, rc::Rc, sync::mpsc};
use zeroize::Zeroizing;

/// Shown in a password field when a key is already stored, signalling "saved"
/// without revealing the real key or its length. Cleared the moment the user
/// focuses the field to type a replacement.
const SAVED_KEY_PLACEHOLDER: &str = "••••••••••••";

pub fn build_settings() -> adw::Window {
    let cfg = crate::load_config_or_warn();

    let hotkey_picker = crate::hotkey_picker::HotkeyPicker::new(&cfg.general.hotkey);
    let lang_row = adw::EntryRow::new();
    lang_row.set_title("Language");
    lang_row.set_text(&cfg.soniox.language);
    let model_row = adw::EntryRow::new();
    model_row.set_title("Model");
    model_row.set_text(&cfg.llm.model);
    crate::model_suggest::attach_model_suggestions(&model_row);
    let split_row = adw::SwitchRow::new();
    split_row.set_title("Split into separate messages");
    split_row.set_subtitle(
        "Sends each natural pause as its own chat message. Off keeps one line — \
         best for terminals and editors.",
    );
    split_row.set_active(cfg.llm.message_splitting);
    let soniox_key_row = adw::PasswordEntryRow::new();
    soniox_key_row.set_title("Soniox API Key");
    let openrouter_key_row = adw::PasswordEntryRow::new();
    openrouter_key_row.set_title("OpenRouter API Key");

    // `true` while the field still shows the saved-key placeholder and the user
    // has not started editing — such a field is never re-saved.
    let soniox_pristine = Rc::new(Cell::new(false));
    let openrouter_pristine = Rc::new(Cell::new(false));
    attach_clear_on_focus(&soniox_key_row, Rc::clone(&soniox_pristine));
    attach_clear_on_focus(&openrouter_key_row, Rc::clone(&openrouter_pristine));

    let general = adw::PreferencesGroup::new();
    general.set_title("General");
    general.add(hotkey_picker.row());

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

    let page = adw::PreferencesPage::new();
    page.add(&general);
    page.add(&dictation);
    page.add(&speech);
    page.add(&llm_grp);

    // Toasts need an overlay host since a plain `adw::Window` — unlike a
    // `PreferencesWindow` — does not surface them on its own.
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&page));

    // Explicit apply lives in the header bar (where the window's primary action
    // belongs), not as a stray button floating inside the scrolled content.
    let apply_btn = gtk4::Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Settings", "")));
    header.pack_end(&apply_btn);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let window = adw::Window::new();
    window.set_title(Some("linux-flow Settings"));
    window.set_default_size(480, 640);
    window.set_content(Some(&toolbar));

    show_saved_placeholders(
        &soniox_key_row,
        Rc::clone(&soniox_pristine),
        &openrouter_key_row,
        Rc::clone(&openrouter_pristine),
    );

    apply_btn.connect_clicked({
        let win = window.clone();
        let toasts = toasts.clone();
        move |_| {
            if let Err(e) = patch_config(|c| {
                c.general.hotkey = hotkey_picker.selected_evdev().to_string();
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
                toasts.add_toast(adw::Toast::new(&format!("Failed to save settings: {e}")));
                return;
            }
            let (tx, rx) = mpsc::channel::<(&'static str, Result<(), String>)>();
            let mut expected: u32 = 0;

            if !soniox_pristine.get() {
                let soniox_v = Zeroizing::new(soniox_key_row.text().to_string());
                if !soniox_v.is_empty() {
                    expected += 1;
                    spawn_save_key("soniox", "Soniox", soniox_v, tx.clone());
                }
            }
            if !openrouter_pristine.get() {
                let or_v = Zeroizing::new(openrouter_key_row.text().to_string());
                if !or_v.is_empty() {
                    expected += 1;
                    spawn_save_key("openrouter", "OpenRouter", or_v, tx.clone());
                }
            }
            drop(tx);
            if expected == 0 {
                win.close();
                return;
            }

            let remaining = Rc::new(Cell::new(expected));
            let win_c = win.clone();
            let toasts_c = toasts.clone();
            gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                loop {
                    match rx.try_recv() {
                        Ok((name, result)) => {
                            remaining.set(remaining.get() - 1);
                            if let Err(e) = result {
                                toasts_c.add_toast(adw::Toast::new(&format!(
                                    "Failed to save {name} key: {e}"
                                )));
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            remaining.set(0);
                            break;
                        }
                    }
                }
                if remaining.get() == 0 {
                    win_c.close();
                    return gtk4::glib::ControlFlow::Break;
                }
                gtk4::glib::ControlFlow::Continue
            });
        }
    });

    window
}

fn patch_config(update: impl FnOnce(&mut Config)) -> Result<(), String> {
    let mut cfg = Config::load().map_err(|e| e.to_string())?;
    update(&mut cfg);
    cfg.save().map_err(|e| e.to_string())
}

/// Clears the saved-key placeholder the first time the field gains focus, so
/// the user types a fresh key into an empty field instead of editing dots.
fn attach_clear_on_focus(row: &adw::PasswordEntryRow, pristine: Rc<Cell<bool>>) {
    let focus = gtk4::EventControllerFocus::new();
    let row_weak = row.downgrade();
    focus.connect_enter(move |_| {
        if pristine.replace(false) {
            if let Some(r) = row_weak.upgrade() {
                r.set_text("");
            }
        }
    });
    row.add_controller(focus);
}

/// Probes libsecret off the GTK thread and fills each field with a fixed dot
/// placeholder when a key is already stored. The real key is never loaded.
fn show_saved_placeholders(
    soniox_row: &adw::PasswordEntryRow,
    soniox_pristine: Rc<Cell<bool>>,
    openrouter_row: &adw::PasswordEntryRow,
    openrouter_pristine: Rc<Cell<bool>>,
) {
    let (tx, rx) = mpsc::channel::<(&'static str, bool)>();
    spawn_check_key("SONIOX_API_KEY", "soniox", tx.clone());
    spawn_check_key("OPENROUTER_API_KEY", "openrouter", tx.clone());
    drop(tx);

    let soniox_row = soniox_row.downgrade();
    let openrouter_row = openrouter_row.downgrade();
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        loop {
            match rx.try_recv() {
                Ok((service, true)) => {
                    let (row, pristine) = if service == "soniox" {
                        (soniox_row.upgrade(), &soniox_pristine)
                    } else {
                        (openrouter_row.upgrade(), &openrouter_pristine)
                    };
                    if let Some(r) = row {
                        r.set_text(SAVED_KEY_PLACEHOLDER);
                        pristine.set(true);
                    }
                }
                Ok((_, false)) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return gtk4::glib::ControlFlow::Break;
                }
            }
        }
        gtk4::glib::ControlFlow::Continue
    });
}

fn spawn_check_key(
    env_var: &'static str,
    service: &'static str,
    tx: mpsc::Sender<(&'static str, bool)>,
) {
    std::thread::spawn(move || {
        let exists = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|rt| {
                rt.block_on(crate::secrets::resolve_key(env_var, service))
                    .is_ok()
            })
            .unwrap_or(false);
        let _ = tx.send((service, exists));
    });
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
