#!/usr/bin/env bash
# linux-flow install script — safe to run via: curl -sSL <url> | bash
set -euo pipefail

REPO_URL="https://github.com/Abdulrahmanch1/linux-flow"
PREFIX="/usr/local"
LIB_DIR="$PREFIX/lib/linux-flow"
SYSTEMD_USER="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
_TMPDIR=""
_GROUPS_CHANGED=0
_INSTALLED_FILES=()

# ── helpers ───────────────────────────────────────────────────────────────────

die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 not found — please install it first"; }
info() { printf '=> %s\n' "$*"; }

# ── rollback ──────────────────────────────────────────────────────────────────

_rollback() {
    local exit_code=$?
    [[ $exit_code -eq 0 ]] && return
    printf '\nerror: installation failed — rolling back installed files...\n' >&2
    for f in "${_INSTALLED_FILES[@]}"; do
        [[ -e "$f" ]] && sudo rm -f "$f" 2>/dev/null || true
    done
    sudo rmdir --ignore-fail-on-non-empty "$LIB_DIR" 2>/dev/null || true
    systemctl --user disable linux-flow.service 2>/dev/null || true
    [[ -n "$_TMPDIR" ]] && rm -rf "$_TMPDIR"
    printf 'Rollback complete. Your system is unchanged.\n' >&2
}
trap '_rollback' EXIT

track() {
    # track <dest> — record a successfully installed file for rollback
    _INSTALLED_FILES+=("$1")
}

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
    info "Cloning linux-flow..."
    git clone --depth 1 "$REPO_URL" "$_TMPDIR/src" >/dev/null
    echo "$_TMPDIR/src"
}

# ── build ─────────────────────────────────────────────────────────────────────

build() {
    local src="$1"
    need cargo
    # Use half the available cores so the build doesn't freeze the system.
    local jobs
    jobs=$(( $(nproc) / 2 ))
    jobs=$(( jobs < 1 ? 1 : jobs ))
    info "Building with $jobs/$( nproc) cores (this may take 5–10 minutes on first build)..."
    local t0
    t0=$(date +%s)
    (cd "$src" && CARGO_BUILD_JOBS="$jobs" cargo build --release)
    local elapsed=$(( $(date +%s) - t0 ))
    info "Build finished in ${elapsed}s."
    [[ -x "$src/target/release/linux-flow" ]]        || die "build output missing: linux-flow"
    [[ -x "$src/target/release/linux-flow-helper" ]] || die "build output missing: linux-flow-helper"
}

# ── install ───────────────────────────────────────────────────────────────────

install_files() {
    local src="$1"
    info "Installing binaries..."
    sudo install -Dm755 "$src/target/release/linux-flow"        "$PREFIX/bin/linux-flow"
    track "$PREFIX/bin/linux-flow"
    sudo install -Dm755 "$src/target/release/linux-flow-helper" "$LIB_DIR/linux-flow-helper"
    track "$LIB_DIR/linux-flow-helper"

    info "Installing system files..."
    sudo install -Dm644 "$src/system/50-linux-flow.rules"        /etc/polkit-1/rules.d/50-linux-flow.rules
    track "/etc/polkit-1/rules.d/50-linux-flow.rules"
    sudo install -Dm644 "$src/system/org.linux-flow.policy"      /usr/share/polkit-1/actions/org.linux-flow.policy
    track "/usr/share/polkit-1/actions/org.linux-flow.policy"
    sudo install -Dm644 "$src/system/60-linux-flow-uinput.rules" /etc/udev/rules.d/60-linux-flow-uinput.rules
    track "/etc/udev/rules.d/60-linux-flow-uinput.rules"
    sudo install -Dm644 "$src/system/linux-flow.desktop"         /usr/share/applications/linux-flow.desktop
    track "/usr/share/applications/linux-flow.desktop"
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

# ── service start ─────────────────────────────────────────────────────────────

start_service() {
    systemctl --user daemon-reload
    systemctl --user enable linux-flow.service

    # Only start now if uinput group is active in the current login session.
    # If not active (group was just added, or user hasn't logged out yet),
    # linux-flow-helper would fail to open /dev/uinput and crash-loop every 2s.
    local uinput_gid
    uinput_gid=$(getent group uinput 2>/dev/null | cut -d: -f3)
    if [[ -n "$uinput_gid" ]] && id -G | tr ' ' '\n' | grep -qx "$uinput_gid"; then
        info "Starting linux-flow service..."
        systemctl --user start linux-flow.service
    else
        info "Service enabled. It will start automatically on your next login."
        [[ $_GROUPS_CHANGED -eq 1 ]] && info "Group membership updated — please log out and back in."
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
        info "Stopping linux-flow service..."
        systemctl --user stop linux-flow.service 2>/dev/null || true
        pkill -TERM -x linux-flow        2>/dev/null || true
        pkill -TERM -x linux-flow-helper 2>/dev/null || true
        sleep 1
    fi

    ensure_tools
    ensure_build_deps

    local src
    src=$(find_src)
    build "$src"
    install_files "$src"
    setup_uinput
    start_service

    # Disarm rollback — installation succeeded.
    trap - EXIT
    [[ -n "$_TMPDIR" ]] && rm -rf "$_TMPDIR"

    info "linux-flow installed successfully."
    update-desktop-database --quiet /usr/share/applications 2>/dev/null || true
}

main "$@"
