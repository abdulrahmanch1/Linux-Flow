use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use std::{cell::RefCell, rc::Rc};
use zeroize::{Zeroize, Zeroizing};

use crate::wizard_flow::{self, SetupNeeds, Step};

pub(crate) struct WizardState {
    pub soniox_key: Zeroizing<String>,
    pub openrouter_key: Zeroizing<String>,
    pub model: String,
    pub hotkey: String,
    pub needs: SetupNeeds,
}

impl WizardState {
    /// Seeds model/hotkey from the existing config when present so a partial
    /// re-run preserves them instead of resetting to defaults. Keys start empty
    /// and are only written for the pieces `needs` says are actually missing.
    pub(crate) fn new(needs: SetupNeeds) -> Self {
        let cfg = linux_flow_shared::config::Config::load().unwrap_or_default();
        Self {
            soniox_key: Zeroizing::new(String::new()),
            openrouter_key: Zeroizing::new(String::new()),
            model: cfg.llm.model,
            hotkey: cfg.general.hotkey,
            needs,
        }
    }
}

/// Build the first-run wizard window, starting at the first missing piece. Call
/// `.present()` to show it.
pub fn build_wizard(needs: SetupNeeds) -> adw::Window {
    let state = Rc::new(RefCell::new(WizardState::new(needs)));
    let nav = adw::NavigationView::new();
    nav.push(&wizard_flow::start_page(Rc::clone(&state), nav.clone()));

    let window = adw::Window::new();
    window.set_title(Some("linux-flow Setup"));
    window.set_default_size(480, 520);
    window.set_content(Some(&nav));
    window
}

pub(crate) fn page_soniox(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    let entry = adw::PasswordEntryRow::new();
    entry.set_title("Soniox API Key");

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    entry.connect_changed(glib::clone!(
        #[weak]
        next,
        move |e| {
            next.set_sensitive(!e.text().is_empty());
        }
    ));

    {
        let mut stored = state.borrow().soniox_key.as_str().to_owned();
        if !stored.is_empty() {
            entry.set_text(&stored);
        }
        stored.zeroize();
    }

    next.connect_clicked(glib::clone!(
        #[weak]
        entry,
        #[weak]
        nav,
        #[strong]
        state,
        move |_| {
            state.borrow_mut().soniox_key = Zeroizing::new(entry.text().to_string());
            wizard_flow::advance(Step::Soniox, Rc::clone(&state), nav.clone());
        }
    ));

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

pub(crate) fn page_openrouter(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    // The key field is shown and required only when the stored OpenRouter key
    // is actually missing; otherwise we keep the present key and collect just
    // the model, so a partial re-run never overwrites a good key with empty.
    let need_key = state.borrow().needs.openrouter_key;

    let key_entry = adw::PasswordEntryRow::new();
    key_entry.set_title("OpenRouter API Key");
    // Always kept in the tree (so its weak refs in the callbacks below stay
    // valid) but hidden when the stored key is still present.
    key_entry.set_visible(need_key);

    let model_entry = adw::EntryRow::new();
    model_entry.set_title("Model");
    let default_model = state.borrow().model.clone();
    model_entry.set_text(&default_model);
    crate::model_suggest::attach_model_suggestions(&model_entry);

    let next = gtk4::Button::with_label("Next");
    next.add_css_class("suggested-action");
    // With no key field, the prefilled model already satisfies the form.
    next.set_sensitive(!need_key && !default_model.is_empty());

    model_entry.connect_changed(glib::clone!(
        #[weak]
        key_entry,
        #[weak]
        next,
        move |e| {
            let key_ok = !need_key || !key_entry.text().is_empty();
            next.set_sensitive(key_ok && !e.text().is_empty());
        }
    ));
    key_entry.connect_changed(glib::clone!(
        #[weak]
        model_entry,
        #[weak]
        next,
        move |e| {
            next.set_sensitive(!e.text().is_empty() && !model_entry.text().is_empty());
        }
    ));

    next.connect_clicked(glib::clone!(
        #[weak]
        key_entry,
        #[weak]
        model_entry,
        #[weak]
        nav,
        #[strong]
        state,
        move |_| {
            if need_key {
                state.borrow_mut().openrouter_key = Zeroizing::new(key_entry.text().to_string());
            }
            state.borrow_mut().model = model_entry.text().to_string();
            wizard_flow::advance(Step::OpenRouter, Rc::clone(&state), nav.clone());
        }
    ));

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
