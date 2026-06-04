use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use zeroize::{Zeroize, Zeroizing};

use crate::key_check::{KeyCheckError, Provider};
use crate::wizard_flow::{self, SetupNeeds, Step};
use crate::wizard_validate;

/// Applies a key-check verdict to the page: a good key advances; a rejected one
/// blocks with a message; an unverifiable one (offline) warns and arms `skip` so
/// the next Next press proceeds without re-checking.
///
/// `reenable` restores the inputs (key field + Next) that were frozen for the
/// duration of the check. It runs on every path — including success — so that
/// navigating back to a verified page never lands on a dead, greyed-out form.
fn apply_verdict(
    verdict: Result<(), KeyCheckError>,
    provider: Provider,
    status: &gtk4::Label,
    skip: &Cell<bool>,
    reenable: &dyn Fn(),
    advance: &dyn Fn(),
) {
    reenable();
    match verdict {
        Ok(()) => {
            status.set_text("");
            advance();
        }
        Err(KeyCheckError::Invalid) => {
            status.set_text(&format!(
                "{} API key is invalid. Check it and try again.",
                provider.label()
            ));
        }
        Err(KeyCheckError::Unreachable(_)) => {
            skip.set(true);
            status.set_text("Couldn't verify the key (offline?). Press Next again to continue.");
        }
    }
}

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

    let status = gtk4::Label::new(None);
    status.set_wrap(true);
    status.set_xalign(0.0);

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    // Set true after an unverifiable (offline) check so the next press proceeds;
    // any edit to the key clears it, forcing a fresh verification.
    let skip = Rc::new(Cell::new(false));

    entry.connect_changed(glib::clone!(
        #[weak]
        next,
        #[strong]
        skip,
        move |e| {
            skip.set(false);
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
        next,
        #[weak]
        status,
        #[weak]
        nav,
        #[strong]
        state,
        #[strong]
        skip,
        move |_| {
            let key = Zeroizing::new(entry.text().trim().to_string());
            state.borrow_mut().soniox_key = key.clone();
            if skip.get() {
                wizard_flow::advance(Step::Soniox, Rc::clone(&state), nav.clone());
                return;
            }
            // Freeze the field too, not just Next: an edit mid-check would
            // re-arm the form and let a stale verdict advance with a key that no
            // longer matches what is shown.
            next.set_sensitive(false);
            entry.set_sensitive(false);
            status.set_text("Verifying key…");
            let advance = {
                let state = Rc::clone(&state);
                let nav = nav.clone();
                move || wizard_flow::advance(Step::Soniox, Rc::clone(&state), nav.clone())
            };
            let reenable = glib::clone!(
                #[weak]
                entry,
                #[weak]
                next,
                move || {
                    entry.set_sensitive(true);
                    next.set_sensitive(true);
                }
            );
            wizard_validate::spawn_check(
                Provider::Soniox,
                key,
                glib::clone!(
                    #[weak]
                    status,
                    #[strong]
                    skip,
                    move |verdict| apply_verdict(
                        verdict,
                        Provider::Soniox,
                        &status,
                        &skip,
                        &reenable,
                        &advance
                    )
                ),
            );
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
    body.append(&status);
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

    let status = gtk4::Label::new(None);
    status.set_wrap(true);
    status.set_xalign(0.0);

    let next = gtk4::Button::with_label("Next");
    next.add_css_class("suggested-action");
    // With no key field, the prefilled model already satisfies the form.
    next.set_sensitive(!need_key && !default_model.is_empty());

    // Set true after an unverifiable (offline) check so the next press proceeds;
    // any edit to the key clears it, forcing a fresh verification.
    let skip = Rc::new(Cell::new(false));

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
        #[strong]
        skip,
        move |e| {
            skip.set(false);
            next.set_sensitive(!e.text().is_empty() && !model_entry.text().is_empty());
        }
    ));

    next.connect_clicked(glib::clone!(
        #[weak]
        key_entry,
        #[weak]
        model_entry,
        #[weak]
        next,
        #[weak]
        status,
        #[weak]
        nav,
        #[strong]
        state,
        #[strong]
        skip,
        move |_| {
            let key = Zeroizing::new(key_entry.text().trim().to_string());
            {
                let mut st = state.borrow_mut();
                if need_key {
                    st.openrouter_key = key.clone();
                }
                st.model = model_entry.text().to_string();
            }
            // No key to check when resuming with a stored key; advance as before.
            if !need_key || skip.get() {
                wizard_flow::advance(Step::OpenRouter, Rc::clone(&state), nav.clone());
                return;
            }
            // Freeze every input, not just Next: an edit mid-check would re-arm
            // the form (the model field's handler re-enables Next) and let a
            // stale verdict — or a second concurrent check — advance, pushing
            // the next page twice.
            next.set_sensitive(false);
            key_entry.set_sensitive(false);
            model_entry.set_sensitive(false);
            status.set_text("Verifying key…");
            let advance = {
                let state = Rc::clone(&state);
                let nav = nav.clone();
                move || wizard_flow::advance(Step::OpenRouter, Rc::clone(&state), nav.clone())
            };
            let reenable = glib::clone!(
                #[weak]
                key_entry,
                #[weak]
                model_entry,
                #[weak]
                next,
                move || {
                    key_entry.set_sensitive(true);
                    model_entry.set_sensitive(true);
                    next.set_sensitive(true);
                }
            );
            wizard_validate::spawn_check(
                Provider::OpenRouter,
                key,
                glib::clone!(
                    #[weak]
                    status,
                    #[strong]
                    skip,
                    move |verdict| apply_verdict(
                        verdict,
                        Provider::OpenRouter,
                        &status,
                        &skip,
                        &reenable,
                        &advance
                    )
                ),
            );
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
    body.append(&status);
    body.append(&next_row);

    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&body));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&clamp));

    adw::NavigationPage::new(&toolbar, "OpenRouter")
}
