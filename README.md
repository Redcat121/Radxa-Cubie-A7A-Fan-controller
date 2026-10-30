# Radxa Cubie A7A Thermal Controller

Keep your **Radxa Cubie A7A** cool automatically.

This small program watches your board's temperature sensors and drives the PWM
fan so the hottest spot stays in a comfortable window — no more manually
adjusting fan speed when the SoC gets hot under load.

## What it does

- **Finds your hottest spot.** It reads every temperature sensor on the board
  (CPU, SoC, GPU) and tracks the hottest one.
- **Smooths it out.** Raw sensor readings are noisy, so the fan adjusts
  gradually instead of jittering on and off.
- **Keeps you quiet when you can.** At low temperatures the fan stays calm.
  Only when things get hot does it ramp up.
- **Protects the hardware.** If the program crashes or you shut the board down,
  it forces all fans to full speed first — so the board never cools down while
  it's dead.

### How it works

| Step | What happens |
|------|--------------|
| **1. Scan** | Looks at `/sys/class/thermal` for temperature sensors and fan devices |
| **2. Find hot-spot** | Reads each sensor (in milli-°C) and finds the highest |
| **3. Smooth it** | Filters the noisy reading so the fan doesn't jitter |
| **4. Avoid chatter** | Only changes fan speed when the temp moves more than 1°C |
| **5. Drive the fan** | Maps temperature → fan speed and writes it to the fan |
| **6. Stay safe** | On shutdown or crash, maxes the fans before exiting |

## Requirements

- A **Radxa Cubie A7A** board with a PWM fan (uses the `pwm_fan` kernel driver)
- **Rust** on the board (via [rustup](https://rustup.rs))
- **Root/sudo** access — the program needs to write to the fan

## Quick start (recommended)

If you want the simplest experience, install a prebuilt binary — this needs no
Rust toolchain or source checkout:

```sh
# 1. Install the controller (downloads the latest prebuilt binary)
sudo curl -sSL https://github.com/Redcat121/Radxa-Cubie-A7A-Fan-controller/releases/latest/download/Radxa-Cubie-A7A-thermal-controller -o /usr/local/bin/thermal-controller
```
```sh
# 2. Marked it as executable
sudo chmod +x /usr/local/bin/thermal-controller
```
```sh
# 3. Download the service file
sudo curl -sSL https://raw.githubusercontent.com/Redcat121/Radxa-Cubie-A7A-Fan-controller/HEAD/thermal-controller.service -o /etc/systemd/system/thermal-controller.service
```

```sh
# 4. Reload systemd so it recognizes the new file
sudo systemctl daemon-reload
```

```sh
# 5. Start it
sudo systemctl start thermal-controller
```

```sh
# 6. Enable it on boot 
sudo systemctl enable thermal-controller
```

```sh
# check to see if it is running
sudo systemctl status thermal-controller
```

At startup the controller waits up to about a minute (12 scans, 5 seconds
apart) for a thermal zone and a fan to appear, because the `pwm_fan` driver can
load late. If none show up it exits with an error, and the service's
`Restart=always` makes systemd try again after 5 seconds.

## Install from source (not tested)

If no prebuilt release exists yet, the installer can build the binary for you
and set everything up:

```sh
sudo bash ./install.sh
```

It will:

1. Install Rust (rustup) if it isn't already there.
2. Build the controller.
3. Put it at `/usr/local/bin/thermal-controller`.
4. Load the `pwm_fan` kernel driver.
5. Set up a systemd service that auto-restarts and shuts down safely.

Useful options:

```sh
bash ./install.sh --dry-run   # preview what it would do, without changing anything
bash ./install.sh --help      # show usage
```

## Build manually

To build the binary yourself:

```sh
cargo build --release
```

This produces `target/release/thermal-controller`.

## Running it

```sh
# Load the PWM fan driver if it isn't already
sudo modprobe pwm_fan

# Make sure the thermal devices were found
ls /sys/class/thermal/

# Run it (needs root to write fan speed)
sudo ./target/release/thermal-controller
```


## Configuration

The controller's behavior is controlled by a few constants near the top of
[`src/main.rs`](src/main.rs):

| Setting | Default | What it means |
|---------|---------|---------------|
| `TEMP_MIN` | 35.0°C | Fan starts ramping up above this |
| `TEMP_MAX` | 65.0°C | Fan goes to full speed above this |
| `EMA_ALPHA` | 0.2 | Smoothing factor — lower is smoother but slower to react |
| `HYSTERESIS_BUFFER` | 1.0°C | Smallest temp change that triggers a fan change |
| `POLL_INTERVAL` | 2s | How often the sensors are re-read |

## Testing

There are unit tests for the core logic that run without any hardware:

```sh
cargo test
```

## Design notes

- **Decoupled control.** The program only writes to the fan interface — it never
  talks to the kernel's thermal control API. That means it plays nicely with
  other tools like `cpufreq` and `congestionctl`.
- **Safe by default.** The release build disables the Rust panic runtime (good
  for embedded systems), but the emergency "max the fans" safety hook still runs.

## Project layout

```
Cargo.toml
README.md          # this file
ARCHITECTURE.md    # how the code works
CONTRIBUTING.md    # how to contribute
LICENSE            # MIT license
install.sh         # installer (one-liner or from source)
thermal-controller.service  # systemd unit (used by the README and install.sh)
src/
├── main.rs        # the controller
└── tests.rs       # (removed; tests now live inline in main.rs)
.gitignore
```

## License

Released under the [MIT License](LICENSE).
