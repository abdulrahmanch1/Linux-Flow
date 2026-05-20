use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use std::{cell::RefCell, rc::Rc};
use zeroize::Zeroizing;

pub(crate) struct WizardState {
    pub soniox_key: Zeroizing<String>,
    pub openrouter_key: Zeroizing<String>,
    pub model: String,
    pub hotkey: String,
}

impl Default for WizardState {
    fn default() -> Self {
        let cfg = linux_flow_shared::config::Config::default();
        Self {
            soniox_key: Zeroizing::new(String::new()),
            openrouter_key: Zeroizing::new(String::new()),
            model: cfg.llm.model,
            hotkey: cfg.general.hotkey,
        }
    }
}

/// Returns `true` if the first-run wizard must be shown.
/// Checks config file presence first, then probes libsecret for both keys.
pub async fn needs_setup() -> bool {
    if linux_flow_shared::config::Config::config_path()
        .map(|p| !p.exists())
        .unwrap_or(true)
    {
        return true;
    }
    crate::secrets::resolve_key("SONIOX_API_KEY", "soniox")
        .await
        .is_err()
        || crate::secrets::resolve_key("OPENROUTER_API_KEY", "openrouter")
            .await
            .is_err()
}

/// Build the first-run wizard window. Call `.present()` to show it.
pub fn build_wizard() -> adw::Window {
    let state = Rc::new(RefCell::new(WizardState::default()));
    let nav = adw::NavigationView::new();
    nav.push(&page_soniox(Rc::clone(&state), nav.clone()));

    let window = adw::Window::new();
    window.set_title(Some("linux-flow Setup"));
    window.set_default_size(480, 520);
    window.set_content(Some(&nav));
    window
}

fn page_soniox(state: Rc<RefCell<WizardState>>, nav: adw::NavigationView) -> adw::NavigationPage {
    let entry = adw::PasswordEntryRow::new();
    entry.set_title("Soniox API Key");

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    entry.connect_changed(glib::clone!(#[weak] next, move |e| {
        next.set_sensitive(!e.text().is_empty());
    }));

    next.connect_clicked(glib::clone!(#[weak] entry, #[weak] nav, #[strong] state, move |_| {
        state.borrow_mut().soniox_key = Zeroizing::new(entry.text().to_string());
        nav.push(&page_openrouter(Rc::clone(&state), nav.clone()));
    }));

    let group = adw::PreferencesGroup::new();
    group.add(&entry);

    let next_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    next_row.set_halign(gtk4::Align::End);
    next_row.append(&next);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(&group);
    body.append(&next_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, "Soniox API Key")
}

fn page_openrouter(state: Rc<RefCell<WizardState>>, nav: adw::NavigationView) -> adw::NavigationPage {
    let key_entry = adw::PasswordEntryRow::new();
    key_entry.set_title("OpenRouter API Key");

    let model_entry = adw::EntryRow::new();
    model_entry.set_title("Model");
    let default_model = state.borrow().model.clone();
    model_entry.set_text(&default_model);

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    key_entry.connect_changed(glib::clone!(#[weak] model_entry, #[weak] next, move |e| {
        next.set_sensitive(!e.text().is_empty() && !model_entry.text().is_empty());
    }));

    model_entry.connect_changed(glib::clone!(#[weak] key_entry, #[weak] next, move |e| {
        next.set_sensitive(!key_entry.text().is_empty() && !e.text().is_empty());
    }));

    next.connect_clicked(glib::clone!(#[weak] key_entry, #[weak] model_entry, #[weak] nav, #[strong] state, move |_| {
        state.borrow_mut().openrouter_key = Zeroizing::new(key_entry.text().to_string());
        state.borrow_mut().model = model_entry.text().to_string();
        nav.push(&crate::wizard_hotkey::page_hotkey(Rc::clone(&state), nav.clone()));
    }));

    let group = adw::PreferencesGroup::new();
    group.add(&key_entry);
    group.add(&model_entry);

    let next_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    next_row.set_halign(gtk4::Align::End);
    next_row.append(&next);

    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    body.set_margin_top(24);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    body.append(&group);
    body.append(&next_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, "OpenRouter")
}
