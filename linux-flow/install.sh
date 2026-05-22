#!/usr/bin/env bash
# linux-flow install script — safe to run via: curl -sSL <url> | bash
set -euo pipefail

REPO_URL="https://github.com/Abdulrahmanch1/linux-flow"
PREFIX="/usr/local"
LIB_DIR="$PREFIX/lib/linux-flow"
SYSTEMD_USER="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
_TMPDIR=""
_GROUPS_CHANGED=0

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
    local changed=0
    sudo groupadd -f uinput
    if ! id -nG "$USER" | grep -qw uinput; then
        sudo usermod -aG uinput "$USER"
        changed=1
    fi
    if ! id -nG "$USER" | grep -qw input; then
        sudo usermod -aG input "$USER"
        changed=1
    fi
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    if [[ $changed -eq 1 ]]; then
        _GROUPS_CHANGED=1
        printf '\n=> Group membership updated. Please log out and back in for changes to take effect.\n'
        printf '   linux-flow will start automatically on your next login.\n'
    fi
}

# ── package helpers ───────────────────────────────────────────────────────────

pkg_install() {
    local pkg="$1"
    info "Installing $pkg..."
    if   command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y "$pkg" >/dev/null
    elif command -v dnf     >/dev/null 2>&1; then sudo dnf     install -y "$pkg" >/dev/null
    elif command -v pacman  >/dev/null 2>&1; then sudo pacman  -S --noconfirm "$pkg" >/dev/null
    else die "$pkg is required but no supported package manager was found — install it manually"
    fi
}

pkg_install_distro() {
    # Usage: pkg_install_distro <label> <apt_pkg> <dnf_pkg> <pacman_pkg>
    local label="$1" apt_pkg="$2" dnf_pkg="$3" pac_pkg="$4"
    info "Installing $label..."
    if   command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y "$apt_pkg" >/dev/null
    elif command -v dnf     >/dev/null 2>&1; then sudo dnf     install -y "$dnf_pkg" >/dev/null
    elif command -v pacman  >/dev/null 2>&1; then sudo pacman  -S --noconfirm "$pac_pkg" >/dev/null
    else die "$label is required but no supported package manager was found — install it manually"
    fi
}

ensure_tools() {
    command -v wl-copy     >/dev/null 2>&1 || pkg_install wl-clipboard
    command -v xclip       >/dev/null 2>&1 || pkg_install xclip
    command -v notify-send >/dev/null 2>&1 || pkg_install_distro "notify-send" libnotify-bin libnotify libnotify
}

ensure_build_deps() {
    if command -v apt-get >/dev/null 2>&1; then
        local pkgs=(libgtk-4-dev libadwaita-1-dev libpipewire-0.3-dev libssl-dev libx11-dev)
        local missing=()
        for p in "${pkgs[@]}"; do dpkg -s "$p" &>/dev/null || missing+=("$p"); done
        if [[ ${#missing[@]} -gt 0 ]]; then
            info "Installing build dependencies: ${missing[*]}"
            sudo apt-get install -y "${missing[@]}" >/dev/null
        fi
    elif command -v dnf >/dev/null 2>&1; then
        info "Installing build dependencies..."
        sudo dnf install -y gtk4-devel libadwaita-devel pipewire-devel openssl-devel libX11-devel >/dev/null
    elif command -v pacman >/dev/null 2>&1; then
        info "Installing build dependencies..."
        sudo pacman -S --noconfirm --needed gtk4 libadwaita pipewire openssl libx11 >/dev/null
    else
        die "no supported package manager — install GTK4, libadwaita, PipeWire, OpenSSL, and X11 dev headers manually"
    fi
}

# ── main ──────────────────────────────────────────────────────────────────────

main() {
    [[ $EUID -eq 0 ]] && die "run as a regular user — this script uses sudo internally"
    need sudo
    need udevadm
    need systemctl

    if [[ -f "$PREFIX/bin/linux-flow" ]]; then
        printf '=> linux-flow is already installed. Update to the latest version? [y/n]: '
        read -r answer
        [[ "${answer,,}" != "y" ]] && { info "Aborted."; exit 0; }
        systemctl --user stop linux-flow.service 2>/dev/null || true
    fi

    ensure_tools
    ensure_build_deps

    local src
    src=$(find_src)
    build "$src"
    install_files "$src"
    setup_uinput

    systemctl --user daemon-reload
    systemctl --user enable linux-flow.service
    if [[ $_GROUPS_CHANGED -eq 0 ]]; then
        systemctl --user start linux-flow.service
    fi

    info "linux-flow installed successfully."
}

main "$@"
