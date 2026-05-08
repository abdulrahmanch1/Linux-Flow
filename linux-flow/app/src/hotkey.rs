pub use linux_flow_shared::display::DisplayServer;

pub fn detect_display() -> DisplayServer {
    linux_flow_shared::display::detect()
}
