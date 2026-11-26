use crate::error::DaemonError;
use std::{fs::File, io::Write, sync::mpsc, time::Duration};
use wayland_client::{
    delegate_noop,
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
    ext_data_control_offer_v1 as offer, ext_data_control_source_v1 as source,
};
use zeroize::Zeroizing;

const MIME_TYPES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];

struct State {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<manager::ExtDataControlManagerV1>,
    text: Zeroizing<String>,
    cancelled: bool,
}

/// Retain the text in memory until another application replaces the selection.
/// A compositor roundtrip, not a sleep, fences ownership before the paste key.
pub fn copy(text: &str) -> Result<(), DaemonError> {
    let text = Zeroizing::new(text.to_owned());
    let (ready, result) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("flow-clipboard".into())
        .spawn(move || match setup(text) {
            Ok((mut queue, mut state)) => {
                // If the caller timed out, relinquish this selection immediately.
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while !state.cancelled {
                    if let Err(e) = queue.blocking_dispatch(&mut state) {
                        eprintln!("linux-flow-helper: clipboard disconnected: {e}");
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = ready.send(Err(e));
            }
        })?;
    result
        .recv_timeout(Duration::from_secs(3))
        .map_err(clipboard_error)?
}

fn clipboard_error(error: impl std::fmt::Display) -> DaemonError {
    DaemonError::InjectionFailed(format!("Wayland clipboard: {error}"))
}

fn setup(text: Zeroizing<String>) -> Result<(EventQueue<State>, State), DaemonError> {
    let conn = Connection::connect_to_env().map_err(clipboard_error)?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State {
        seat: None,
        manager: None,
        text,
        cancelled: false,
    };
    queue.roundtrip(&mut state).map_err(clipboard_error)?;
    let seat = state.seat.as_ref().ok_or_else(|| clipboard_error("no seat"))?;
    let manager = state
        .manager
        .as_ref()
        .ok_or_else(|| clipboard_error("ext_data_control_manager_v1 is unavailable"))?;
    let source = manager.create_data_source(&qh, ());
    for mime in MIME_TYPES {
        source.offer(mime.into());
    }
    let device = manager.get_data_device(seat, &qh, ());
    device.set_selection(Some(&source));
    queue.roundtrip(&mut state).map_err(clipboard_error)?;
    if state.cancelled {
        return Err(clipboard_error("selection was cancelled before paste"));
    }
    Ok((queue, state))
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_data_control_manager_v1" => {
                    state.manager = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<source::ExtDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &source::ExtDataControlSourceV1,
        event: source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            source::Event::Send { mime_type, fd } => {
                if MIME_TYPES.contains(&mime_type.as_str()) {
                    // Every request gets the complete text; reads do not consume it.
                    // Broken pipes are normal when a requesting client cancels.
                    let _ = File::from(fd).write_all(state.text.as_bytes());
                }
            }
            source::Event::Cancelled => state.cancelled = true,
            _ => {}
        }
    }
}

impl Dispatch<device::ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &device::ExtDataControlDeviceV1,
        event: device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            device::Event::Selection { id: Some(id) }
            | device::Event::PrimarySelection { id: Some(id) } => id.destroy(),
            device::Event::Finished => state.cancelled = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, device::ExtDataControlDeviceV1, [
        0 => (offer::ExtDataControlOfferV1, ())
    ]);
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: manager::ExtDataControlManagerV1);
delegate_noop!(State: ignore offer::ExtDataControlOfferV1);
