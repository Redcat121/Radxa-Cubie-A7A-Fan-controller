#!/usr/bin/env bash
#
# install.sh — Install the thermal-controller daemon onto a Radxa Cubie A7A.
#
# Two modes:
#
#   1. Prebuilt binary (recommended, one-liner after setup):
#        curl -sSL https://github.com/Redcat121/Radxa-Cubie-A7A-thermal-controller/releases/download/latest/Radxa-Cubie-A7A-thermal-controller | sudo tee /usr/local/bin/thermal-controller
#
#   2. Build from source (no prebuilt release available):
#        bash ./install.sh --from-source
#
# Steps (source mode):
#   1. Install the Rust toolchain (rustup) if it is missing.
#   2. Build the release binary.
#   3. Load the pwm_fan kernel module (enables /sys/class/thermal cooling_device*).
#   4. Install the systemd service unit (thermal-controller.service, auto-restart).
#
# Usage:
#   sudo bash ./install.sh                       # from-source install
#   sudo bash ./install.sh --from-source         # from-source (explicit)
#   bash ./install.sh --dry-run                  # preview only, no changes
#   bash ./install.sh --help                     # usage
#
# Exit codes: 0 = success, non-zero = error.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="thermal-controller"
FROM_SOURCE=0
DRY_RUN=0
RELEASE_URL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --from-source) FROM_SOURCE=1; shift ;;
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
        printf '  \033[33m[dry-run] would run: %s\n' "$*"
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
# From-source build
# ---------------------------------------------------------------------------
if [[ $FROM_SOURCE -eq 1 ]]; then
    section "Build from source"
    if ! command -v cargo >/dev/null 2>&1; then
        info "cargo not found; installing Rust via rustup..."
        run curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "stable"
        run bash -lc 'echo "source \$HOME/.cargo/env" >> ~/.bashrc'
    fi
    BIN="target/release/$SERVICE_NAME"
    run cargo build --release
    if [[ ! -x "$BIN" ]]; then
        err "Build did not produce $BIN."
        exit 1
    fi
    log "Built $BIN"
else
    section "Fetch prebuilt binary from GitHub Release"
    if [[ -z "$RELEASE_URL" ]]; then
        err "No RELEASE_URL set; build from source instead:  bash ./install.sh --from-source"
        exit 1
    fi
    info "Downloading from: $RELEASE_URL"
    run curl -sSL "$RELEASE_URL" -o "$SERVICE_NAME.tmp"
    if [[ ! -s "$SERVICE_NAME.tmp" ]]; then
        err "Download was empty."
        exit 1
    fi
fi

# ---------------------------------------------------------------------------
# Install binary
# ---------------------------------------------------------------------------
section "Install binary"
if [[ ! -d "$INSTALL_DIR" ]]; then
    run mkdir -p "$INSTALL_DIR"
fi
run chmod +x "$SERVICE_NAME.tmp"
run cp "$SERVICE_NAME.tmp" "$INSTALL_DIR/$SERVICE_NAME"
log "Installed $INSTALL_DIR/$SERVICE_NAME"

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
