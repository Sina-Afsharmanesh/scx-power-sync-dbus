# scx-power-sync-dbus

Event-driven **sched_ext (SCX) scheduler binder** for **power-profiles-daemon**.

`scx-power-sync-dbus` runs in the background, watches your current power profile over D-Bus (from `power-profiles-daemon`), and automatically **starts/switches** the active SCX scheduler using `scxctl`.

This is useful if you want different sched_ext schedulers (or different arguments) for:
- `performance` (plugged in / maximum performance)
- `balanced` (default)
- `power-saver` (battery / efficiency)

---

## Quick start

1) Install prerequisites:

- `power-profiles-daemon` (D-Bus service `net.hadess.PowerProfiles`)
- `scx_loader` + `scxctl` (sched_ext loader + CLI)
- a kernel with `sched_ext` enabled

2) Create a config:

```bash
mkdir -p ~/.config/scx-power-sync-dbus
cp contrib/scx-power-sync-dbus.yaml ~/.config/scx-power-sync-dbus/config.yaml
# edit to match your schedulers/args
$EDITOR ~/.config/scx-power-sync-dbus/config.yaml
```

3) Run in a terminal (so you can see logs):

```bash
RUST_LOG=info scx-power-sync-dbus
```

4) Flip power profiles and watch it switch schedulers:

```bash
powerprofilesctl set power-saver
powerprofilesctl set balanced
powerprofilesctl set performance
```

---

## How it works

1. Connects to the **system D-Bus** service:
   - Destination: `net.hadess.PowerProfiles`
   - Path: `/net/hadess/PowerProfiles`
   - Interface: `net.hadess.PowerProfiles`

2. Reads the `ActiveProfile` property once at startup.
3. Subscribes to `org.freedesktop.DBus.Properties.PropertiesChanged`.
4. When `ActiveProfile` changes, it applies the matching scheduler definition from your config:

   - Runs `scxctl get` to check whether an SCX scheduler is running.
   - If none is running → `scxctl start --sched <sched> --args=<args>`
   - If one is running → `scxctl switch --sched <sched> --args=<args>`

Notes:
- The program passes the entire `args` payload as a **single argument**: `--args=<args>`.
- Profile names in config must match `power-profiles-daemon` naming: `performance`, `balanced`, `power-saver`.
- If `scxctl get` fails, the daemon assumes “not running” (and attempts a `start`) while logging a warning.

---

## Requirements

### Runtime

- `power-profiles-daemon` (provides `net.hadess.PowerProfiles` on the system bus)  
  D-Bus API docs: https://hadess.fedorapeople.org/power-profiles-daemon-docs/gdbus-net.hadess.PowerProfiles.html
- `scxctl` in `PATH` (used to start/switch sched_ext schedulers)  
  `scx_loader` and `scxctl`: https://github.com/sched-ext/scx-loader  
  `scxctl` help/examples: https://github.com/frap129/scxctl
- `powerprofilesctl` in `PATH` (checked at startup; also useful for debugging)  
  Man page: https://manpages.debian.org/unstable/power-profiles-daemon/powerprofilesctl.1

### Kernel / sched_ext

- A Linux kernel with `sched_ext` support  
  Kernel docs: https://docs.kernel.org/scheduler/sched-ext.html  
  SCX project: https://github.com/sched-ext/scx

### Permissions / polkit

Starting/switching sched_ext schedulers is typically a privileged operation.

Depending on your system configuration, `scxctl` may:
- require `sudo`, **or**
- trigger a polkit authentication prompt (via `scx_loader`).

**Important for systemd --user:** if a polkit prompt is required, it must be handled by a polkit agent in your session. If you run the daemon as a user service *without* a polkit agent, scheduler switching may fail.

---

## Installation

### Build from source

```bash
git clone https://github.com/Sina-Afsharmanesh/scx-power-sync-dbus
cd scx-power-sync-dbus
cargo build --release
sudo install -Dm755 target/release/scx-power-sync-dbus /usr/bin/scx-power-sync-dbus
```

### Arch Linux (PKGBUILD)

This repo includes a `PKGBUILD` that:
- builds a release binary
- installs a **systemd user** unit
- installs a default config to `/etc/xdg/scx-power-sync-dbus/config.yaml`

```bash
git clone https://github.com/Sina-Afsharmanesh/scx-power-sync-dbus
cd scx-power-sync-dbus
makepkg -si
```

---

## Configuration

Config is YAML with a single top-level key: `modes`.

### Schema

```yaml
modes:
  performance:
    sched: <string>
    args: <string>
  balanced:
    sched: <string>
    args: <string>
  power-saver:
    sched: <string>
    args: <string>
```

All three profiles are **required**. If any is missing, the program exits with an error.

### Example

The repo ships an example under `contrib/scx-power-sync-dbus.yaml`:

```yaml
modes:
  performance:
    sched: flash
    args: "-m all -L -C 90 -s 6000 -r 65536"
  balanced:
    sched: lavd
    args: "--autopilot"
  power-saver:
    sched: flash
    args: "-m powersave -I 8000 -t 5000 -s 8000 -S 500"
```

### Config search paths

At startup it searches, in order:

1. `$HOME/.config/scx-power-sync-dbus/config.yaml`
2. `$XDG_CONFIG_HOME/scx-power-sync-dbus/config.yaml`
3. Each entry in `$XDG_CONFIG_DIRS`:
   - `<dir>/scx-power-sync-dbus/config.yaml`
4. If `XDG_CONFIG_DIRS` is unset:
   - `/etc/xdg/scx-power-sync-dbus/config.yaml`
5. Fallback:
   - `/etc/scx-power-sync-dbus/config.yaml`

---

## Running

### Manual (recommended first)

```bash
scx-power-sync-dbus
```

Increase verbosity:

```bash
RUST_LOG=debug scx-power-sync-dbus
```

### Example: run as root via sudo

If your `scxctl` requires elevation and you don't want to deal with polkit agents:

```bash
sudo -E RUST_LOG=info scx-power-sync-dbus
```

---

## systemd

A hardened **user** unit is included at `contrib/scx-power-sync-dbus.service`.

### User service

Enable/start:

```bash
systemctl --user daemon-reload
systemctl --user enable --now scx-power-sync-dbus.service
```

Logs:

```bash
journalctl --user -u scx-power-sync-dbus -f
```

### If polkit prompting is required

If `scxctl` triggers polkit authentication and your user service fails because no agent is available,
either:
- ensure a polkit agent is running in your session, or
- run it as a **system service** (root).

A simple system service variant:

```ini
# /etc/systemd/system/scx-power-sync-dbus.service
[Unit]
Description=Sync SCX scheduler with power-profiles-daemon over D-Bus
After=dbus.service
Wants=dbus.service

[Service]
Type=simple
ExecStart=/usr/bin/scx-power-sync-dbus
Restart=on-failure
RestartSec=2
User=root

[Install]
WantedBy=multi-user.target
```

Enable/start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now scx-power-sync-dbus.service
sudo journalctl -u scx-power-sync-dbus -f
```

### Service hardening notes

The included unit uses common systemd hardening directives like:
- `NoNewPrivileges=yes`
- `PrivateTmp=yes`
- `ProtectSystem=strict`
- `ProtectHome=read-only`

Docs:
- https://www.freedesktop.org/software/systemd/man/systemd.exec.html
- https://wiki.archlinux.org/title/Systemd/Sandboxing

---

## Confirmation & verification

### 1) Confirm the power-profiles-daemon D-Bus API is available

```bash
busctl --system list | grep -F net.hadess.PowerProfiles
```

Read the active profile:

```bash
busctl --system get-property   net.hadess.PowerProfiles   /net/hadess/PowerProfiles   net.hadess.PowerProfiles   ActiveProfile
```

Or with `gdbus`:

```bash
gdbus call --system   --dest net.hadess.PowerProfiles   --object-path /net/hadess/PowerProfiles   --method org.freedesktop.DBus.Properties.Get   net.hadess.PowerProfiles ActiveProfile
```

### 2) Confirm `powerprofilesctl` works

```bash
powerprofilesctl list
powerprofilesctl get
powerprofilesctl set balanced
```

### 3) Confirm `scxctl` can control sched_ext

```bash
scxctl list
scxctl get
```

Try a manual start/switch:

```bash
scxctl start --sched lavd --args="--autopilot"
scxctl switch --sched flash --args="-m powersave"
```

### 4) Confirm the daemon reacts to profile changes

Run in one terminal:

```bash
RUST_LOG=info scx-power-sync-dbus
```

In another:

```bash
powerprofilesctl set power-saver
powerprofilesctl set balanced
powerprofilesctl set performance
```

Then check scheduler state:

```bash
scxctl get
```

---

## Common failures

### “configuration file not found; looked in: ...”
Create the config in one of the search paths:

```bash
mkdir -p ~/.config/scx-power-sync-dbus
cp contrib/scx-power-sync-dbus.yaml ~/.config/scx-power-sync-dbus/config.yaml
```

### “required binary not found in PATH: scxctl / powerprofilesctl”
Install the missing binaries and ensure `PATH` is correct for your environment.
(systemd user units often have a smaller PATH than your interactive shell.)

### “connect system D-Bus” / cannot reach `net.hadess.PowerProfiles`
Ensure `power-profiles-daemon` is running and exporting on the **system bus**:

```bash
systemctl status power-profiles-daemon
busctl --system list | grep -F net.hadess.PowerProfiles
```

### “unknown power profile: …”
This daemon only accepts: `performance`, `balanced`, `power-saver`.

### “scxctl start/switch failed …”
Most often:
- not authorized (needs sudo/polkit)
- scx_loader not running / not configured
- scheduler name not installed/known
- invalid args for that scheduler

Reproduce by running the same `scxctl ...` command manually.

### User service works manually, but not via `systemctl --user`
Common causes:
- different `PATH` under systemd
- no polkit agent available to answer authentication prompts

Solutions:
- use absolute paths or set `Environment=PATH=...` in the unit
- run a polkit agent
- run as a system service (root)

---

## Security notes

This tool delegates privileged operations to `scxctl` / `scx_loader` + your system policy.
Even with systemd hardening, the real boundary is whatever `scx_loader`/polkit (or sudo) enforces.

---

## Project layout

- `src/main.rs` — daemon logic (D-Bus watcher + `scxctl` executor)
- `contrib/`
  - `scx-power-sync-dbus.service` — systemd user unit
  - `scx-power-sync-dbus.yaml` — example config
- `PKGBUILD` — Arch packaging

---

## License

MIT (see `LICENSE`).
