use crate::wayland_clipboard::State;
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1 as handle, zcosmic_toplevel_info_v1 as info,
};
use std::collections::HashMap;
use wayland_client::{
    backend::ObjectId, protocol::wl_registry, Connection, Dispatch, Proxy, QueueHandle,
};

struct Window {
    handle: handle::ZcosmicToplevelHandleV1,
    terminal: bool,
    active: bool,
}

pub(super) struct FocusTracker {
    enabled: bool,
    manager: Option<info::ZcosmicToplevelInfoV1>,
    windows: HashMap<ObjectId, Window>,
}

impl FocusTracker {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            manager: None,
            windows: HashMap::new(),
        }
    }

    pub fn bind(&mut self, registry: &wl_registry::WlRegistry, name: u32, qh: &QueueHandle<State>) {
        if self.enabled {
            // Version 1 provides app_id and activation together, without joining
            // separate foreign-toplevel and COSMIC object lifetimes.
            self.manager = Some(registry.bind(name, 1, qh, ()));
        }
    }

    pub fn finish(&mut self) -> bool {
        let terminal = self
            .windows
            .values()
            .any(|window| window.active && window.terminal);
        self.enabled = false;
        // Detection is a snapshot at paste time, not a background window monitor.
        if let Some(manager) = self.manager.take() {
            manager.stop();
        }
        for (_, window) in self.windows.drain() {
            window.handle.destroy();
        }
        terminal
    }
}

fn is_terminal(app_id: &str) -> bool {
    // Exact desktop IDs, not window titles or substring matching: a browser tab
    // or editor document named "terminal" must retain its ordinary paste key.
    let name = app_id.rsplit('.').next().unwrap_or(app_id);
    [
        "ptyxis",
        "cosmic-term",
        "CosmicTerm",
        "gnome-terminal",
        "Terminal",
        "Console",
        "kgx",
        "konsole",
        "kitty",
        "Alacritty",
        "wezterm",
        "foot",
        "footclient",
        "xterm",
        "UXTerm",
        "URxvt",
        "st",
        "xfce4-terminal",
        "mate-terminal",
        "terminator",
        "terminology",
        "tilix",
        "guake",
        "tilda",
        "yakuake",
        "BlackBox",
    ]
    .iter()
    .any(|terminal| name.eq_ignore_ascii_case(terminal))
}

impl Dispatch<info::ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &info::ZcosmicToplevelInfoV1,
        event: info::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let info::Event::Toplevel { toplevel } = event {
            if !state.focus.enabled {
                toplevel.destroy();
                return;
            }
            state.focus.windows.insert(
                toplevel.id(),
                Window {
                    handle: toplevel,
                    terminal: false,
                    active: false,
                },
            );
        }
    }
    wayland_client::event_created_child!(State, info::ZcosmicToplevelInfoV1, [
        0 => (handle::ZcosmicToplevelHandleV1, ())
    ]);
}

impl Dispatch<handle::ZcosmicToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &handle::ZcosmicToplevelHandleV1,
        event: handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let handle::Event::Closed = event {
            if let Some(window) = state.focus.windows.remove(&handle.id()) {
                window.handle.destroy();
            }
            return;
        }
        let Some(window) = state.focus.windows.get_mut(&handle.id()) else {
            return;
        };
        match event {
            handle::Event::AppId { app_id } => window.terminal = is_terminal(&app_id),
            handle::Event::State { state } => {
                window.active = state.chunks_exact(4).any(|value| {
                    u32::from_ne_bytes([value[0], value[1], value[2], value[3]])
                        == handle::State::Activated as u32
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_terminal;

    #[test]
    fn recognizes_native_and_flatpak_terminal_ids() {
        for app_id in [
            "app.devsuite.Ptyxis",
            "org.gnome.Ptyxis",
            "com.system76.CosmicTerm",
            "org.gnome.Terminal",
            "org.kde.konsole",
            "Alacritty",
            "net.kovidgoyal.kitty",
        ] {
            assert!(is_terminal(app_id), "{app_id}");
        }
    }

    #[test]
    fn does_not_use_terminal_shortcuts_for_editors_or_similar_names() {
        for app_id in [
            "firefox",
            "org.mozilla.firefox",
            "signal",
            "antigravity",
            "code",
            "org.example.TerminalNotes",
            "not-a-terminal",
            "",
        ] {
            assert!(!is_terminal(app_id), "{app_id}");
        }
    }
}
