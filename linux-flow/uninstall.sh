#!/usr/bin/env bash
# linux-flow uninstall script
# Usage: uninstall.sh [--purge]
#   default  Remove binaries and system files; preserve ~/.config/linux-flow
#   --purge  Also remove config directory and clear API keys from the keyring
set -uo pipefail

PURGE=0

for arg in "$@"; do
    case "$arg" in
        --purge) PURGE=1 ;;
        *) printf 'error: unknown option: %s\n' "$arg" >&2; exit 1 ;;
    esac
done

PREFIX="/usr/local"
LIB_DIR="$PREFIX/lib/linux-flow"
SYSTEMD_USER="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/linux-flow"

info()    { printf '=> %s\n' "$*"; }
removed() { printf '   removed: %s\n' "$*"; }
skipped() { printf '   skipped (not found): %s\n' "$*"; }
warn()    { printf 'warning: %s\n' "$*" >&2; }

confirm() {
    printf '%s [y/N]: ' "$1"
    local ans
    read -r ans
    [[ "${ans,,}" == "y" ]]
}

rm_file() {
    # rm_file <sudo|user> <path>
    local as="$1" path="$2"
    if [[ -e "$path" ]]; then
        if [[ "$as" == "sudo" ]]; then sudo rm -f "$path"; else rm -f "$path"; fi
        removed "$path"
    else
        skipped "$path"
    fi
}

stop_app() {
    info "Stopping linux-flow..."
    systemctl --user stop    linux-flow.service 2>/dev/null || true
    systemctl --user disable linux-flow.service 2>/dev/null || true
    systemctl --user daemon-reload              2>/dev/null || true
    pkill -TERM -x linux-flow        2>/dev/null || true
    pkill -TERM -x linux-flow-helper 2>/dev/null || true
    sleep 1
    pkill -KILL -x linux-flow        2>/dev/null || true
    pkill -KILL -x linux-flow-helper 2>/dev/null || true
}

remove_files() {
    info "Removing binaries..."
    rm_file sudo "$PREFIX/bin/linux-flow"
    rm_file sudo "$LIB_DIR/linux-flow-helper"
    sudo rmdir --ignore-fail-on-non-empty "$LIB_DIR" 2>/dev/null || true

    info "Removing system files..."
    rm_file sudo /etc/polkit-1/rules.d/50-linux-flow.rules
    rm_file sudo /usr/share/polkit-1/actions/org.linux-flow.policy
    rm_file sudo /etc/udev/rules.d/60-linux-flow-uinput.rules
    rm_file sudo /etc/modules-load.d/linux-flow-uinput.conf
    rm_file sudo /usr/share/applications/linux-flow.desktop

    info "Removing user files..."
    rm_file user "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/linux-flow.desktop"
    rm_file user "$SYSTEMD_USER/linux-flow.service"
}

reload_system() {
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    if command -v pkaction >/dev/null 2>&1; then
        sudo systemctl reload polkit 2>/dev/null || true
    fi
    update-desktop-database --quiet /usr/share/applications 2>/dev/null || true
}

purge_data() {
    info "Purging configuration..."
    if [[ -d "$CONFIG_DIR" ]]; then
        rm -rf "$CONFIG_DIR"
        removed "$CONFIG_DIR"
    else
        skipped "$CONFIG_DIR"
    fi

    info "Clearing API keys from keyring..."
    if command -v secret-tool >/dev/null 2>&1; then
        secret-tool clear service soniox     2>/dev/null; removed "keyring: soniox key"
        secret-tool clear service openrouter 2>/dev/null; removed "keyring: openrouter key"
    else
        warn "secret-tool not found — API keys in the keyring were NOT removed."
        warn "To remove them manually, install libsecret-tools and run:"
        warn "  secret-tool clear service soniox"
        warn "  secret-tool clear service openrouter"
    fi
}

remove_groups() {
    local changed=0
    if id -nG "$USER" 2>/dev/null | grep -qw uinput; then
        if confirm "Remove $USER from the 'uinput' group? (safe unless another app needs it)"; then
            sudo gpasswd -d "$USER" uinput && changed=1
        fi
    fi
    if id -nG "$USER" 2>/dev/null | grep -qw input; then
        if confirm "Remove $USER from the 'input' group? (safe unless another app needs it)"; then
            sudo gpasswd -d "$USER" input && changed=1
        fi
    fi
    [[ $changed -eq 1 ]] && warn "Group changes take effect after you log out and back in."
}

main() {
    if [[ $PURGE -eq 1 ]]; then
        printf 'This will remove linux-flow, all configuration, and stored API keys.\n'
    else
        printf 'This will remove linux-flow binaries and system files.\n'
        printf 'Config at %s will be preserved. Use --purge to also remove it.\n' "$CONFIG_DIR"
    fi
    confirm "Proceed?" || { info "Aborted."; exit 0; }

    stop_app
    remove_files
    reload_system
    [[ $PURGE -eq 1 ]] && purge_data
    remove_groups

    printf '\n'
    info "linux-flow uninstalled."
    [[ $PURGE -eq 0 ]] && info "Config preserved at $CONFIG_DIR — rerun with --purge to remove it."
    info "If the app icon is still visible in the launcher, log out and back in."
}

main "$@"
