#!/usr/bin/env bash
# linux-flow install script — safe to run via: curl -sSL <url> | bash
set -euo pipefail

REPO_URL="https://github.com/Abdulrahmanch1/linux-flow"
PREFIX="/usr/local"
LIB_DIR="$PREFIX/lib/linux-flow"
SYSTEMD_USER="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
_TMPDIR=""

# ── helpers ───────────────────────────────────────────────────────────────────

die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 not found — please install it first"; }
info() { printf '=> %s\n' "$*"; }

# ── source location ───────────────────────────────────────────────────────────

# Running from inside a checkout? Use it directly.
# Running via curl-pipe? Clone from GitHub.
find_src() {
    local dir
    dir="$(cd "$(dirname "${BASH_SOURCE[0]:-/dev/null}")" 2>/dev/null && pwd || echo "")"
    if [[ -f "$dir/Cargo.toml" && -d "$dir/system" ]]; then
        echo "$dir"
        return
    fi
    need git
    _TMPDIR=$(mktemp -d)
    trap 'rm -rf "$_TMPDIR"' EXIT
    info "Cloning linux-flow..."
    git clone --depth 1 "$REPO_URL" "$_TMPDIR/src" >/dev/null
    echo "$_TMPDIR/src"
}

# ── build ─────────────────────────────────────────────────────────────────────

build() {
    local src="$1"
    need cargo
    info "Building (this may take a few minutes)..."
    (cd "$src" && cargo build --release --quiet)
}

# ── install ───────────────────────────────────────────────────────────────────

install_files() {
    local src="$1"
    info "Installing binaries..."
    sudo install -Dm755 "$src/target/release/linux-flow"        "$PREFIX/bin/linux-flow"
    sudo install -Dm755 "$src/target/release/linux-flow-helper" "$LIB_DIR/linux-flow-helper"

    info "Installing system files..."
    sudo install -Dm644 "$src/system/60-linux-flow-uinput.rules" /etc/udev/rules.d/60-linux-flow-uinput.rules
    sudo install -Dm644 "$src/system/linux-flow.desktop"         /usr/share/applications/linux-flow.desktop
    install     -Dm644 "$src/system/linux-flow.desktop"         "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/linux-flow.desktop"
    install     -Dm644 "$src/system/linux-flow.service"         "$SYSTEMD_USER/linux-flow.service"
}

# ── system setup ──────────────────────────────────────────────────────────────

setup_uinput() {
    local groups_changed=0
    sudo groupadd -f uinput
    if ! id -nG "$USER" | grep -qw uinput; then
        sudo usermod -aG uinput "$USER"
        groups_changed=1
    fi
    if ! id -nG "$USER" | grep -qw input; then
        sudo usermod -aG input "$USER"
        groups_changed=1
    fi
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    if [[ $groups_changed -eq 1 ]]; then
        printf '\n=> Group membership updated. Please log out and back in for changes to take effect.\n'
        printf '   linux-flow will start automatically on your next login.\n'
    fi
}

enable_service() {
    systemctl --user daemon-reload
    systemctl --user enable linux-flow.service
}

# ── main ──────────────────────────────────────────────────────────────────────

pkg_install() {
    local pkg="$1"
    info "Installing $pkg..."
    if   command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y "$pkg" >/dev/null
    elif command -v dnf     >/dev/null 2>&1; then sudo dnf     install -y "$pkg" >/dev/null
    elif command -v pacman  >/dev/null 2>&1; then sudo pacman  -S --noconfirm "$pkg" >/dev/null
    else die "$pkg is required but no supported package manager was found — install it manually"
    fi
}

ensure_clipboard_tool() {
    if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
        command -v wl-copy >/dev/null 2>&1 || pkg_install wl-clipboard
    else
        command -v xclip   >/dev/null 2>&1 || pkg_install xclip
    fi
}

main() {
    need sudo
    need udevadm
    need systemctl
    ensure_clipboard_tool

    local src
    src=$(find_src)
    build "$src"
    install_files "$src"
    setup_uinput
    enable_service

    info "linux-flow installed successfully."
}

main "$@"
