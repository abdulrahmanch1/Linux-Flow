#!/usr/bin/env bash
# linux-flow uninstall script
set -euo pipefail

PREFIX="/usr/local"
SYSTEMD_USER="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

info() { printf '=> %s\n' "$*"; }

stop_service() {
    if systemctl --user is-active --quiet linux-flow.service 2>/dev/null; then
        info "Stopping linux-flow service..."
        systemctl --user stop linux-flow.service
    fi
    if systemctl --user is-enabled --quiet linux-flow.service 2>/dev/null; then
        systemctl --user disable linux-flow.service
    fi
    systemctl --user daemon-reload
}

remove_files() {
    info "Removing binaries..."
    sudo rm -f "$PREFIX/bin/linux-flow"
    sudo rm -f "$PREFIX/lib/linux-flow/linux-flow-helper"
    sudo rmdir --ignore-fail-on-non-empty "$PREFIX/lib/linux-flow" 2>/dev/null || true

    info "Removing system files..."
    sudo rm -f /etc/udev/rules.d/60-linux-flow-uinput.rules
    sudo rm -f /usr/share/applications/linux-flow.desktop
    rm  -f "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/linux-flow.desktop"
    rm  -f "$SYSTEMD_USER/linux-flow.service"
}

reload_services() {
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    update-desktop-database --quiet /usr/share/applications 2>/dev/null || true
}

main() {
    stop_service
    remove_files
    reload_services
    info "linux-flow uninstalled. Config in ~/.config/linux-flow was left intact."
}

main "$@"
