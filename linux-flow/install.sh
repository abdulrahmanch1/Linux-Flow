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
    if [[ -x "$src/target/release/linux-flow" && -x "$src/target/release/linux-flow-helper" ]]; then
        return
    fi
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
    sudo install -Dm644 "$src/system/50-linux-flow.rules"        /etc/polkit-1/rules.d/50-linux-flow.rules
    sudo install -Dm644 "$src/system/60-linux-flow-uinput.rules" /etc/udev/rules.d/60-linux-flow-uinput.rules
    sudo install -Dm644 "$src/system/linux-flow.desktop"         /usr/share/applications/linux-flow.desktop
    install     -Dm644 "$src/system/linux-flow.desktop"         "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/linux-flow.desktop"
    install     -Dm644 "$src/system/linux-flow.service"         "$SYSTEMD_USER/linux-flow.service"
}

# ── system setup ──────────────────────────────────────────────────────────────

setup_uinput() {
    sudo groupadd -f uinput
    if ! id -nG "$USER" | grep -qw uinput; then
        sudo usermod -aG uinput "$USER"
        info "Added $USER to uinput group — a relogin is required."
    fi
    if ! id -nG "$USER" | grep -qw input; then
        sudo usermod -aG input "$USER"
        info "Added $USER to input group — a relogin is required."
    fi
    sudo udevadm control --reload-rules
    sudo udevadm trigger
}

enable_service() {
    systemctl --user daemon-reload
    systemctl --user enable --now linux-flow.service
}

# ── main ──────────────────────────────────────────────────────────────────────

main() {
    need sudo
    need udevadm
    need systemctl

    local src
    src=$(find_src)
    build "$src"
    install_files "$src"
    setup_uinput
    enable_service

    info "linux-flow installed successfully."
}

main "$@"
