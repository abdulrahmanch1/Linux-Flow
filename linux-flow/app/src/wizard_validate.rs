use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk4::glib;
use libadwaita as adw;
use zeroize::Zeroizing;

use crate::key_check::{self, KeyCheckError, Provider};
use crate::wizard::WizardState;
use crate::wizard_flow::{self, Step};

/// Freezes the page inputs, runs the key check off the GTK thread, and applies
/// the verdict: a good key advances to `step`'s successor; a rejected one blocks
/// with a message; an unverifiable one (offline) warns and arms `skip` so the
/// next press proceeds. `set_inputs(false)` freezes every editable field plus
/// Next as one unit — the single point that makes a mid-check edit (which could
/// otherwise re-arm the form and double-advance) impossible to leave open.
pub(crate) fn launch_verification(
    provider: Provider,
    key: Zeroizing<String>,
    status: gtk4::Label,
    set_inputs: Rc<dyn Fn(bool)>,
    skip: Rc<Cell<bool>>,
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) {
    set_inputs(false);
    status.set_text("Verifying key…");

    // Each provider's key is collected on exactly one page, so the page to
    // advance from is fully determined by the provider.
    let step = match provider {
        Provider::Soniox => Step::Soniox,
        Provider::OpenRouter => Step::OpenRouter,
    };
    let advance = {
        let state = Rc::clone(&state);
        let nav = nav.clone();
        move || wizard_flow::advance(step, Rc::clone(&state), nav.clone())
    };
    let reenable = {
        let set_inputs = Rc::clone(&set_inputs);
        move || set_inputs(true)
    };

    spawn_check(
        provider,
        key,
        glib::clone!(
            #[weak]
            status,
            #[strong]
            skip,
            move |verdict| apply_verdict(verdict, provider, &status, &skip, &reenable, &advance)
        ),
    );
}

/// Applies a key-check verdict. `reenable` restores the inputs frozen for the
/// check on every path — including success — so navigating back to a verified
/// page never lands on a dead, greyed-out form.
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

/// Runs a key check off the GTK thread and delivers the verdict back on it.
///
/// Mirrors the spawn-thread + current-thread-runtime + `glib::timeout_add_local`
/// poll pattern used by `wizard_complete`/`wizard_polkit`, so the UI never
/// blocks while the network round-trip is in flight. The key is moved into the
/// worker thread and zeroized there (on drop and inside `key_check::check`).
pub(crate) fn spawn_check(
    provider: Provider,
    key: Zeroizing<String>,
    on_result: impl Fn(Result<(), KeyCheckError>) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), KeyCheckError>>();
    std::thread::spawn(move || {
        let outcome = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(key_check::check(provider, key)),
            Err(e) => Err(KeyCheckError::Unreachable(e.to_string())),
        };
        let _ = tx.send(outcome);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                on_result(result);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            // Worker dropped the sender without a result (panic / unexpected
            // exit); fold it into the unreachable path so the user isn't blocked.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                on_result(Err(KeyCheckError::Unreachable(
                    "verification worker exited without a result".to_owned(),
                )));
                glib::ControlFlow::Break
            }
        }
    });
}
