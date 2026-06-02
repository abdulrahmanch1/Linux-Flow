use std::{cell::RefCell, rc::Rc, sync::mpsc};

use gtk4::{glib, prelude::*};
use libadwaita as adw;

const MAX_SUGGESTIONS: usize = 8;

/// Attaches a live suggestion popover to a model-name entry. The field stays
/// freely editable; suggestions come from OpenRouter's public model catalog,
/// fetched once off the GTK thread and kept in memory only. If the fetch fails
/// (e.g. no network) the field simply works without suggestions.
pub fn attach_model_suggestions(entry: &adw::EntryRow) {
    let ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.set_can_focus(false);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_max_content_height(240);
    scroller.set_propagate_natural_height(true);
    scroller.set_child(Some(&list));

    // Autohide off so popping up never steals focus from the entry: the user
    // keeps typing while suggestions update underneath.
    let popover = gtk4::Popover::new();
    popover.set_autohide(false);
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
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
        list,
        #[strong]
        ids,
        move |e| {
            refilter(&list, &popover, &e.text(), &ids.borrow());
        }
    ));

    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));
    entry.add_controller(focus);

    spawn_fetch(ids);
}

fn refilter(list: &gtk4::ListBox, popover: &gtk4::Popover, text: &str, ids: &[String]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let needle = text.trim().to_lowercase();
    // Nothing to suggest for an empty field or when the text is already an exact id.
    if needle.is_empty() || ids.iter().any(|id| id == text) {
        popover.popdown();
        return;
    }

    let mut shown = 0;
    for id in ids.iter().filter(|id| id.to_lowercase().contains(&needle)) {
        let label = gtk4::Label::new(Some(id));
        label.set_xalign(0.0);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        label.set_margin_start(8);
        label.set_margin_end(8);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&label));
        row.set_can_focus(false);
        list.append(&row);

        shown += 1;
        if shown >= MAX_SUGGESTIONS {
            break;
        }
    }

    if shown == 0 {
        popover.popdown();
    } else {
        popover.popup();
    }
}

fn spawn_fetch(ids: Rc<RefCell<Vec<String>>>) {
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            if let Ok(list) = rt.block_on(crate::models::fetch_model_ids()) {
                let _ = tx.send(list);
            }
        }
    });

    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        match rx.try_recv() {
            Ok(list) => {
                *ids.borrow_mut() = list;
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}
