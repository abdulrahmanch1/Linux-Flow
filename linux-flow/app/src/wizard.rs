use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use std::{cell::RefCell, rc::Rc};
use zeroize::Zeroizing;

pub(crate) struct WizardState {
    pub soniox_key: Zeroizing<String>,
    pub openrouter_key: Zeroizing<String>,
    pub model: String,
}

impl Default for WizardState {
    fn default() -> Self {
        Self {
            soniox_key: Zeroizing::new(String::new()),
            openrouter_key: Zeroizing::new(String::new()),
            model: linux_flow_shared::config::Config::default().llm.model,
        }
    }
}

/// Build the first-run wizard window. Call `.present()` to show it.
pub fn build_wizard() -> adw::Window {
    let state = Rc::new(RefCell::new(WizardState::default()));
    let nav = adw::NavigationView::new();
    nav.push(&page_soniox(Rc::clone(&state)));

    let window = adw::Window::new();
    window.set_title(Some("linux-flow Setup"));
    window.set_default_size(480, 520);
    window.set_content(Some(&nav));
    window
}

fn page_soniox(state: Rc<RefCell<WizardState>>) -> adw::NavigationPage {
    let entry = adw::PasswordEntryRow::new();
    entry.set_title("Soniox API Key");

    let next = gtk4::Button::with_label("Next");
    next.set_sensitive(false);
    next.add_css_class("suggested-action");

    entry.connect_changed(glib::clone!(#[weak] next, move |e| {
        next.set_sensitive(!e.text().is_empty());
    }));

    next.connect_clicked(glib::clone!(#[weak] entry, #[strong] state, move |_| {
        state.borrow_mut().soniox_key = Zeroizing::new(entry.text().to_string());
        // page_openrouter will be pushed here
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
