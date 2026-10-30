#!/usr/bin/env bash
#
# install.sh — Install the thermal-controller daemon onto a Radxa Cubie A7A.
#
# Two modes:
#
#   1. Build from source (default):
#        sudo bash ./install.sh
#
#   2. Download the prebuilt binary from the latest GitHub Release:
#        sudo bash ./install.sh --prebuilt
#
# Steps:
#   1. Source mode: install the Rust toolchain (rustup) if it is missing and
#      build the release binary. Prebuilt mode: download the release asset.
#   2. Install the binary to /usr/local/bin/thermal-controller.
#   3. Load the pwm_fan kernel module (enables /sys/class/thermal cooling_device*).
#   4. Install the systemd service unit (thermal-controller.service, auto-restart).
#
# Usage:
#   sudo bash ./install.sh                       # build from source (default)
#   sudo bash ./install.sh --prebuilt            # download the prebuilt binary
#   bash ./install.sh --dry-run                  # preview only, no changes
#   bash ./install.sh --help                     # usage
#
# Exit codes: 0 = success, non-zero = error.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="thermal-controller"               # installed binary and systemd unit name
BIN_NAME="Radxa-Cubie-A7A-thermal-controller"   # cargo binary and release asset name
RELEASE_URL="https://github.com/Redcat121/Radxa-Cubie-A7A-Fan-controller/releases/latest/download/$BIN_NAME"
PREBUILT=0
DRY_RUN=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prebuilt)    PREBUILT=1; shift ;;
        --from-source) PREBUILT=0; shift ;;   # kept for compatibility; this is the default
        --dry-run)     DRY_RUN=1; shift ;;
        -h|--help)
            grep -E '^#( |$)|^Usage' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $1 (try --help)" >&2; exit 1 ;;
    esac
done

log()  { printf '  \033[32m[✔]\033[0m %s\n' "$*"; }
info() { printf '  \033[36m[ℹ]\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m[!] \033[0m %s\n' "$*"; }
err()  { printf '  \033[31m[✘]\033[0m %s\n' "$*" >&2; }

section() {
    printf '\n\033[1m\033[35m▶ %s\033[0m\n' "$*"
}

# In dry-run mode, print but never execute.
run() {
    if [[ $DRY_RUN -eq 1 ]]; then
        printf '  \033[33m[dry-run] would run: %s\033[0m\n' "$*"
    else
        "$@"
    fi
}

# ---------------------------------------------------------------------------
# Detect the board architecture (should be aarch64 on the Cubie A7A).
# ---------------------------------------------------------------------------
ARCH="$(uname -m)"
if [[ "$ARCH" != "aarch64" ]]; then
    err "Unexpected architecture: $ARCH (expected aarch64 for the Cubie A7A)."
    err "This binary is built for the Allwinner A733 (aarch64). If your board"
    err "is a different arch, rebuild with the matching target."
    exit 1
fi

# ---------------------------------------------------------------------------
# Header
# ---------------------------------------------------------------------------
echo "=== Thermal Controller Installer (Radxa Cubie A7A) ==="
if [[ $DRY_RUN -eq 1 ]]; then
    echo "DRY-RUN MODE: nothing will be modified."
fi
echo

# ---------------------------------------------------------------------------
# Get the binary: download it or build it. Either way STAGED_BIN ends up
# pointing at the file to install.
# ---------------------------------------------------------------------------
if [[ $PREBUILT -eq 1 ]]; then
    section "Fetch prebuilt binary from GitHub Release"
    STAGED_BIN="${TMPDIR:-/tmp}/$BIN_NAME.download"
    info "Downloading from: $RELEASE_URL"
    run curl -fsSL "$RELEASE_URL" -o "$STAGED_BIN"
    if [[ $DRY_RUN -eq 0 && ! -s "$STAGED_BIN" ]]; then
        err "Download was empty."
        exit 1
    fi
else
    section "Build from source"
    if ! command -v cargo >/dev/null 2>&1; then
        info "cargo not found; installing Rust via rustup..."
        run bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable'
        if [[ -f "$HOME/.cargo/env" ]]; then
            # shellcheck disable=SC1091
            source "$HOME/.cargo/env"
        fi
    fi
    STAGED_BIN="$SCRIPT_DIR/target/release/$BIN_NAME"
    run cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
    if [[ $DRY_RUN -eq 0 && ! -x "$STAGED_BIN" ]]; then
        err "Build did not produce $STAGED_BIN."
        exit 1
    fi
    log "Built $STAGED_BIN"
fi

# ---------------------------------------------------------------------------
# Install binary
# ---------------------------------------------------------------------------
section "Install binary"
run install -D -m 755 "$STAGED_BIN" "$INSTALL_DIR/$SERVICE_NAME"
log "Installed $INSTALL_DIR/$SERVICE_NAME"
if [[ $PREBUILT -eq 1 ]]; then
    run rm -f "$STAGED_BIN"
fi

# ---------------------------------------------------------------------------
# PWM fan driver
# ---------------------------------------------------------------------------
section "PWM fan driver"
if command -v modprobe >/dev/null 2>&1 && ! lsmod 2>/dev/null | grep -q pwm_fan; then
    info "Loading pwm_fan kernel module..."
    run sudo modprobe pwm_fan
    log "pwm_fan loaded."
else
    log "pwm_fan already loaded (or modprobe unavailable)."
fi

echo "  Verifying /sys/class/thermal..."
if ls /sys/class/thermal/cooling_device* >/dev/null 2>&1; then
    log "Found cooling device(s) under /sys/class/thermal/."
else
    warn "No cooling_device* under /sys/class/thermal/ yet."
    warn "Try: sudo modprobe pwm_fan   then   ls /sys/class/thermal/"
fi

# ---------------------------------------------------------------------------
# systemd service (optional)
# ---------------------------------------------------------------------------
section "systemd service"
if command -v systemctl >/dev/null 2>&1 && [[ $DRY_RUN -eq 0 ]]; then
    UNIT="/etc/systemd/system/$SERVICE_NAME.service"
    cp "$SCRIPT_DIR/$SERVICE_NAME.service" "$UNIT"
    run systemctl daemon-reload
    run systemctl enable "$SERVICE_NAME"
    log "Enabled $SERVICE_NAME at boot."
    info "Start/stop:  systemctl start|stop $SERVICE_NAME"
    info "Status:      systemctl status $SERVICE_NAME"
else
    info "Skipping systemd service."
fi

echo
log "Installation complete."
echo
echo "  Manual run:  sudo $INSTALL_DIR/$SERVICE_NAME"
echo
warn "NOTE: this daemon should run under sudo so it can write fan state."
warn "       On shutdown, systemd will force all fans to max before the box powers off."
echo
