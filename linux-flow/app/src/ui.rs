use gtk4::{glib, prelude::*};
use linux_flow_shared::config::Config;

pub fn open_settings() {
    glib::idle_add_once(|| {
        let config = Config::load().unwrap_or_default();
        crate::settings::build_settings(config).present();
    });
}

pub fn open_about() {
    glib::idle_add_once(|| {
        let w = libadwaita::AboutWindow::new();
        w.set_application_name("linux-flow");
        w.set_version(env!("CARGO_PKG_VERSION"));
        w.set_comments("Wispr Flow-style voice dictation for Linux.");
        w.set_license_type(gtk4::License::Gpl30);
        w.present();
    });
}
