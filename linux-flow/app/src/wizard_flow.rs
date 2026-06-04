use std::{cell::RefCell, rc::Rc};

use libadwaita as adw;

use crate::wizard::WizardState;

/// Which pieces of setup are missing, probed independently. Lets the wizard
/// collect only what is gone instead of forcing a full re-entry — and lets the
/// finish step avoid overwriting a still-present key with an empty value.
#[derive(Clone, Copy)]
pub(crate) struct SetupNeeds {
    pub soniox_key: bool,
    pub openrouter_key: bool,
    pub config: bool,
}

impl SetupNeeds {
    pub fn any(self) -> bool {
        self.soniox_key || self.openrouter_key || self.config
    }

    /// The OpenRouter page also carries the model (which lives in config), so it
    /// is needed when either the key or the config file is missing.
    fn needs_step(self, step: Step) -> bool {
        match step {
            Step::Soniox => self.soniox_key,
            Step::OpenRouter => self.openrouter_key || self.config,
            Step::Hotkey => self.config,
            Step::Polkit => true,
        }
    }
}

/// Probes the config file and both keyring entries independently, so "never set
/// up" and "lost one piece" are distinguished rather than collapsed into one.
pub(crate) async fn setup_needs() -> SetupNeeds {
    let config = linux_flow_shared::config::Config::config_path()
        .map(|p| !p.exists())
        .unwrap_or(true);
    let soniox_key = crate::secrets::resolve_key("SONIOX_API_KEY", "soniox")
        .await
        .is_err();
    let openrouter_key = crate::secrets::resolve_key("OPENROUTER_API_KEY", "openrouter")
        .await
        .is_err();
    SetupNeeds {
        soniox_key,
        openrouter_key,
        config,
    }
}

/// Wizard pages in fixed order. Pages whose data is already present are skipped,
/// but the final authorize/write page always runs so the config gets written.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Step {
    Soniox,
    OpenRouter,
    Hotkey,
    Polkit,
}

const ORDER: [Step; 4] = [Step::Soniox, Step::OpenRouter, Step::Hotkey, Step::Polkit];

/// First page to show. Never `Polkit` alone in practice: the wizard only opens
/// when `any()` is true, so at least one earlier page is needed.
fn first_step(needs: SetupNeeds) -> Step {
    ORDER
        .into_iter()
        .find(|&s| needs.needs_step(s))
        .unwrap_or(Step::Polkit)
}

/// First needed page strictly after `after`, else the final page.
fn next_step(after: Step, needs: SetupNeeds) -> Step {
    let start = ORDER.iter().position(|&s| s == after).map_or(0, |i| i + 1);
    ORDER[start..]
        .iter()
        .copied()
        .find(|&s| needs.needs_step(s))
        .unwrap_or(Step::Polkit)
}

fn page_for(
    step: Step,
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    match step {
        Step::Soniox => crate::wizard::page_soniox(state, nav),
        Step::OpenRouter => crate::wizard::page_openrouter(state, nav),
        Step::Hotkey => crate::wizard_hotkey::page_hotkey(state, nav),
        Step::Polkit => crate::wizard_polkit::page_polkit(state, nav),
    }
}

/// Builds the first page to show for the state's needs.
pub(crate) fn start_page(
    state: Rc<RefCell<WizardState>>,
    nav: adw::NavigationView,
) -> adw::NavigationPage {
    let needs = state.borrow().needs;
    page_for(first_step(needs), state, nav)
}

/// Pushes the next needed page after `current`, skipping any whose data is
/// already present. Each page calls this from its primary button.
pub(crate) fn advance(current: Step, state: Rc<RefCell<WizardState>>, nav: adw::NavigationView) {
    let needs = state.borrow().needs;
    let next = next_step(current, needs);
    nav.push(&page_for(next, state, nav.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(soniox_key: bool, openrouter_key: bool, config: bool) -> SetupNeeds {
        SetupNeeds {
            soniox_key,
            openrouter_key,
            config,
        }
    }

    #[test]
    fn fresh_install_walks_every_page_in_order() {
        let n = needs(true, true, true);
        assert!(first_step(n) == Step::Soniox);
        assert!(next_step(Step::Soniox, n) == Step::OpenRouter);
        assert!(next_step(Step::OpenRouter, n) == Step::Hotkey);
        assert!(next_step(Step::Hotkey, n) == Step::Polkit);
    }

    #[test]
    fn only_soniox_missing_skips_straight_to_finish() {
        let n = needs(true, false, false);
        assert!(first_step(n) == Step::Soniox);
        assert!(next_step(Step::Soniox, n) == Step::Polkit);
    }

    #[test]
    fn only_openrouter_key_missing_starts_at_openrouter() {
        let n = needs(false, true, false);
        assert!(first_step(n) == Step::OpenRouter);
        assert!(next_step(Step::OpenRouter, n) == Step::Polkit);
    }

    #[test]
    fn missing_config_collects_model_and_hotkey() {
        // Config holds model (OpenRouter page) and hotkey, so both run even
        // though both keys are still present.
        let n = needs(false, false, true);
        assert!(first_step(n) == Step::OpenRouter);
        assert!(next_step(Step::OpenRouter, n) == Step::Hotkey);
        assert!(next_step(Step::Hotkey, n) == Step::Polkit);
    }

    #[test]
    fn soniox_and_config_missing_skips_only_present_pieces() {
        let n = needs(true, false, true);
        assert!(first_step(n) == Step::Soniox);
        assert!(next_step(Step::Soniox, n) == Step::OpenRouter);
        assert!(next_step(Step::OpenRouter, n) == Step::Hotkey);
    }
}
