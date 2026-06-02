use crate::{
    error::DaemonError,
    server::KeyInjector,
    text_injector::TextInjector,
    uinput_inner::{
        Inner, InputId, UinputSetup, ALL_KEYS, BUS_VIRTUAL, UI_DEV_CREATE, UI_DEV_DESTROY,
        UI_DEV_SETUP, UI_SET_EVBIT, UI_SET_KEYBIT,
    },
};
use libc::c_int;
use linux_flow_shared::display::{detect, DisplayServer};
use std::{fs::OpenOptions, os::unix::io::AsRawFd, sync::Mutex, thread, time::Duration};

pub struct UinputInjector {
    inner: Mutex<Inner>,
    injector: Mutex<(DisplayServer, TextInjector)>,
}

impl UinputInjector {
    pub fn new() -> Result<Self, DaemonError> {
        let fd = {
            let mut last_err = std::io::Error::from(std::io::ErrorKind::NotFound);
            let mut found = None;
            for _ in 0..4 {
                match OpenOptions::new().write(true).open("/dev/uinput") {
                    Ok(f) => {
                        found = Some(f);
                        break;
                    }
                    Err(e) => {
                        last_err = e;
                        thread::sleep(Duration::from_millis(500));
                    }
                }
            }
            found.ok_or(DaemonError::Io(last_err))?
        };
        let raw = fd.as_raw_fd();

        macro_rules! ioctl1 {
            ($req:expr, $arg:expr) => {{
                // SAFETY: fd is open and valid; argument type matches the ioctl spec.
                let r = unsafe { libc::ioctl(raw, $req, $arg as c_int) };
                if r < 0 {
                    return Err(DaemonError::Io(std::io::Error::last_os_error()));
                }
            }};
        }

        ioctl1!(UI_SET_EVBIT, crate::keys::EV_KEY);
        ioctl1!(UI_SET_EVBIT, crate::keys::EV_SYN);
        for &code in ALL_KEYS {
            ioctl1!(UI_SET_KEYBIT, code);
        }

        let mut setup = UinputSetup {
            id: InputId {
                bustype: BUS_VIRTUAL,
                vendor: 0,
                product: 0,
                version: 0,
            },
            name: [0u8; 80],
            ff_effects_max: 0,
        };
        let label = b"linux-flow-helper";
        setup.name[..label.len()].copy_from_slice(label);

        // SAFETY: fd valid; UinputSetup is repr(C) with correct size for this ioctl.
        let r = unsafe { libc::ioctl(raw, UI_DEV_SETUP, &setup as *const UinputSetup) };
        if r < 0 {
            return Err(DaemonError::Io(std::io::Error::last_os_error()));
        }

        // SAFETY: fd valid; UI_DEV_CREATE takes no extra argument.
        let r = unsafe { libc::ioctl(raw, UI_DEV_CREATE) };
        if r < 0 {
            return Err(DaemonError::Io(std::io::Error::last_os_error()));
        }

        thread::sleep(Duration::from_millis(20));
        let display = detect();
        let injector = TextInjector::new(display);
        Ok(Self {
            inner: Mutex::new(Inner { fd }),
            injector: Mutex::new((display, injector)),
        })
    }
}

impl KeyInjector for UinputInjector {
    fn inject_text(&self, text: &str, x11_window: Option<u64>) -> Result<(), DaemonError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| DaemonError::InjectionFailed("mutex poisoned".into()))?;
        let mut inj = self
            .injector
            .lock()
            .map_err(|_| DaemonError::InjectionFailed("mutex poisoned".into()))?;
        // Re-detect on every call: cheap (env var + optional stat), and lets the
        // injector upgrade from Uinput to Wayland if the compositor became available
        // after the daemon started (WAYLAND_DISPLAY was absent at startup).
        let current = detect();
        if current != inj.0 {
            inj.0 = current;
            inj.1 = TextInjector::new(current);
        }
        inj.1.fire(&mut inner, text, x11_window)
    }
}

impl Drop for UinputInjector {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut() {
            // SAFETY: fd is valid; UI_DEV_DESTROY removes the virtual input device.
            unsafe {
                libc::ioctl(inner.fd.as_raw_fd(), UI_DEV_DESTROY);
            }
        }
    }
}
