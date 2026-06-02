use gtk4::{glib, prelude::*};
use libadwaita::{self as adw, prelude::*};
use linux_flow_shared::hotkey::{index_of, SUPPORTED_HOTKEYS};

/// A push-to-talk key picker: a dropdown of supported keys plus a "Detect"
/// button that captures the next pressed key and validates it is a viable
/// push-to-talk key (a modifier or F13–F24, never a character key).
pub struct HotkeyPicker {
    row: adw::ComboRow,
}

impl HotkeyPicker {
    pub fn new(current: &str) -> Self {
        let labels: Vec<&str> = SUPPORTED_HOTKEYS.iter().map(|(_, l)| *l).collect();
        let model = gtk4::StringList::new(&labels);

        let row = adw::ComboRow::new();
        row.set_title("Push-to-Talk Hotkey");
        row.set_model(Some(&model));
        row.set_selected(index_of(current).unwrap_or(0) as u32);

        let detect = gtk4::Button::with_label("Detect");
        detect.set_valign(gtk4::Align::Center);
        detect.add_css_class("flat");
        detect.set_focusable(true);
        row.add_suffix(&detect);

        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(glib::clone!(
            #[weak]
            row,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, _, _| {
                let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
                match crate::wizard_hotkey::gdk_to_evdev(&name) {
                    Some(evdev) => {
                        if let Some(idx) = index_of(evdev) {
                            row.set_selected(idx as u32);
                            row.set_subtitle(&format!("Captured: {}", SUPPORTED_HOTKEYS[idx].1));
                        }
                    }
                    None => row
                        .set_subtitle("Not a valid push-to-talk key — use a modifier or F13–F24."),
                }
                glib::Propagation::Stop
            }
        ));
        detect.add_controller(key_ctrl);

        detect.connect_clicked(glib::clone!(
            #[weak]
            row,
            move |b| {
                row.set_subtitle("Press a key…");
                b.grab_focus();
            }
        ));

        Self { row }
    }

    /// The widget to add into a `PreferencesGroup`.
    pub fn row(&self) -> &adw::ComboRow {
        &self.row
    }

    /// The evdev name of the currently selected key, ready to persist.
    pub fn selected_evdev(&self) -> &'static str {
        let idx = self.row.selected() as usize;
        SUPPORTED_HOTKEYS
            .get(idx)
            .map(|(name, _)| *name)
            .unwrap_or(SUPPORTED_HOTKEYS[0].0)
    }
}
