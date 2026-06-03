use std::{cell::RefCell, rc::Rc, sync::mpsc};

use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};

const MAX_SUGGESTIONS: usize = 8;

/// Attaches a live suggestion popover to a model-name entry. The field stays
/// freely editable; suggestions come from OpenRouter's public model catalog,
/// fetched once off the GTK thread and kept in memory only. If the fetch fails
/// (e.g. no network) the field stays usable for manual entry and a dimmed
/// warning affix appears so the empty list reads as "offline", not "broken".
pub fn attach_model_suggestions(entry: &adw::EntryRow) {
    let ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.set_can_focus(false);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_max_content_height(200);
    scroller.set_propagate_natural_height(true);
    scroller.set_child(Some(&list));

    // Autohide off so popping up never steals focus from the entry: the user
    // keeps typing while suggestions update underneath.
    let popover = gtk4::Popover::new();
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.add_css_class("menu");
    popover.set_child(Some(&scroller));
    popover.set_parent(entry);

    list.connect_row_activated(glib::clone!(
        #[weak]
        entry,
        #[weak]
        popover,
        move |_, row| {
            if let Some(label) = row.child().and_downcast::<gtk4::Label>() {
                entry.set_text(&label.text());
                entry.set_position(-1);
            }
            popover.popdown();
        }
    ));

    entry.connect_changed(glib::clone!(
        #[weak]
        popover,
        #[weak]
        scroller,
        #[weak]
        list,
        #[strong]
        ids,
        move |e| {
            if refilter(&list, &e.text(), &ids.borrow()) == 0 {
                popover.popdown();
            } else {
                anchor_below(&popover, &scroller, e);
                popover.popup();
            }
        }
    ));

    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));
    entry.add_controller(focus);

    spawn_fetch(entry, ids);
}

/// Marks the entry with a dimmed warning affix when the catalog could not be
/// loaded, so an empty suggestion list reads as "offline" rather than "broken".
/// The field stays fully usable—the model id can always be typed by hand.
fn flag_unavailable(entry: &adw::EntryRow) {
    let icon = gtk4::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_tooltip_text(Some(
        "Couldn't load the model list (check your connection or API status). \
         You can still type the model id manually.",
    ));
    icon.add_css_class("dim-label");
    entry.add_suffix(&icon);
}

/// Pins the popover to the entry's bottom edge and matches its width, so the
/// list always drops downward and spans the field instead of floating above it
/// at an arbitrary width.
fn anchor_below(popover: &gtk4::Popover, scroller: &gtk4::ScrolledWindow, entry: &adw::EntryRow) {
    // Match the field width (with a sane floor in case the row is not yet
    // allocated) so the list spans the field and lines up with its edges
    // instead of floating narrow and centred.
    let width = entry.width().max(360);
    scroller.set_size_request(width, -1);
    // Point at the whole field rather than just its bottom edge: dropping down
    // it clears the field's bottom, and when GTK must flip it up near a screen
    // edge it clears the field's top — never overlapping the input either way.
    let field = gtk4::gdk::Rectangle::new(0, 0, width, entry.height());
    popover.set_pointing_to(Some(&field));
}

/// Rebuilds the suggestion rows for the current text and returns how many were
/// shown. Returns 0 for an empty field or when the text already equals an id.
fn refilter(list: &gtk4::ListBox, text: &str, ids: &[String]) -> usize {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let needle = text.trim().to_lowercase();
    if needle.is_empty() || ids.iter().any(|id| id == text) {
        return 0;
    }

    let mut shown = 0;
    for id in ids.iter().filter(|id| id.to_lowercase().contains(&needle)) {
        let label = gtk4::Label::new(Some(id));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        label.set_margin_start(12);
        label.set_margin_end(12);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&label));
        row.set_can_focus(false);
        list.append(&row);

        shown += 1;
        if shown >= MAX_SUGGESTIONS {
            break;
        }
    }

    shown
}

fn spawn_fetch(entry: &adw::EntryRow, ids: Rc<RefCell<Vec<String>>>) {
    let (tx, rx) = mpsc::channel::<Result<Vec<String>, String>>();
    std::thread::spawn(move || {
        // Collapse every failure mode (runtime build, network, bad response)
        // into a single string so the GTK side can surface one hint uniformly.
        let outcome = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt
                .block_on(crate::models::fetch_model_ids())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(outcome);
    });

    glib::timeout_add_local(
        std::time::Duration::from_millis(80),
        glib::clone!(
            #[weak]
            entry,
            #[strong]
            ids,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || match rx.try_recv() {
                Ok(Ok(list)) => {
                    *ids.borrow_mut() = list;
                    glib::ControlFlow::Break
                }
                Ok(Err(msg)) => {
                    tracing::warn!("model list fetch failed: {msg}");
                    flag_unavailable(&entry);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                // Worker dropped the sender without a result (panic / unexpected
                // exit); treat it like any other failure rather than hanging.
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::warn!("model list fetch worker exited without a result");
                    flag_unavailable(&entry);
                    glib::ControlFlow::Break
                }
            }
        ),
    );
}
