# Architecture

This document explains how the thermal controller works and why it's designed this way.

## Overview

The controller is a single-threaded daemon that runs a continuous loop:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Read thermal_zone*/temp  → find hottest zone (°C)          │
│ 2. EMA-smooth that reading (filter noise)                     │
│ 3. Hysteresis gate (avoid fan chatter)                        │
│ 4. Map temp → fan step, write each fan/cur_state              │
│ 5. Sleep 2s, repeat                                          │
└─────────────────────────────────────────────────────────────┘
```

## Key design decisions

### 1. Decoupled control (no kernel `ioctl`)

The controller only ever **writes** to the sysfs fan interface
(`/sys/class/thermal/cooling_device*/cur_state`). It never opens the thermal
interface with an `ioctl` (the kernel's control API). This means it doesn't
fight with other tools:

- **cpufreq** scales CPU power states.
- **congestionctl** does kernel-side thermal throttling.
- **thermald** manages thermal zone setpoints.

Because the controller only writes fan state, it can run alongside all of them
without conflict.

### 2. EMA smoothing

Raw thermal readings are noisy. The controller filters them with an
exponential moving average:

```rust
smoothed = ALPHA * raw + (1 - ALPHA) * smoothed_prev   // ALPHA = 0.2
```

Lower `ALPHA` = smoother but slower to react. Higher = snappier but noisier.

### 3. Hysteresis

The temperature the fan is driven by only updates when it moves more than
`HYSTERESIS_BUFFER` (1°C) since the last applied value. This prevents the
DCM2588 PWM fan (only ~12 discrete steps) from rapidly flapping between two
adjacent steps when the sensor hovers near a threshold.

### 4. Emergency panic hook

If the binary panics, all fans are maxed immediately. This ensures the hardware
never overheats while the SoC is dead. The release build uses `panic = "abort"`
for embedded safety, but the panic hook still runs on abort.

## Data model

- **`ThermalZone`** — a discovered thermal zone (`name`, `temp` path).
- **`CoolingDevice`** — a discovered PWM fan (`name`, `cur_state` path, `max_state`).

## File layout

```
src/main.rs              # the controller (single binary)
Cargo.toml               # package manifest, release profile, dependencies
install.sh               # installer (one-liner + from-source)
README.md                # user-facing docs
ARCHITECTURE.md          # design explanation (this file)
CONTRIBUTING.md          # how to contribute
LICENSE                  # MIT license
ANALYSIS.md              # internal code review (git-ignored)
ci.yml                   # GitHub Actions build workflow
```

## The `panic = "abort"` choice

The release profile sets `panic = "abort"` because unwind frames are large and
slow on embedded targets. Since we handle shutdown manually (maxing fans, then
exiting), we don't need the unwind runtime. The panic hook is registered before
the loop starts, so it still maxes the fans on an abort.

## Why a daemon

Thermal control is a continuous task. The controller runs as a background
service (systemd) and stays alive. On shutdown, it forces all fans to max
first, so the hardware is never left cold during power-off.
