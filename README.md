# Spotlight Linux

A fast Spotlight-style launcher for Linux, built with Rust, GTK 4, and libadwaita.

**Preview release. Tested on Ubuntu 26.04 with GNOME.** Global shortcut capture,
focus/placement and login integration still require the [manual acceptance checks](docs/manual-test-plan.md).

![Spotlight Linux launcher](assets/screenshots/launcher.webp)

## Features

- Fast application search and launch
- Built-in calculator
- Keyboard-first navigation
- Customizable appearance, sizing, and shortcuts
- Optional launch at login
- Native GTK 4 / libadwaita interface

## Screenshots

<p align="center">
  <img src="assets/screenshots/search.webp" width="49%" alt="Application search">
  <img src="assets/screenshots/calculator.webp" width="49%" alt="Calculator">
</p>

<p align="center">
  <img src="assets/screenshots/settings.webp" width="55%" alt="Spotlight Linux settings">
</p>

## Install

### Ready-built preview

Download the archive and `SHA256SUMS` from [Releases](https://github.com/SHADOWOKX/Spotlight-linux/releases).
The binary targets **Ubuntu 26.04, x86_64** and needs the native GTK/libadwaita
runtime; it does not need Rust. In the download directory:

```bash
sha256sum -c SHA256SUMS
tar -xzf spotlight-linux-0.1.1-preview.1-linux-x86_64.tar.gz
cd spotlight-linux-0.1.1-preview.1-linux-x86_64
./install.sh
```

Run as your normal user. Use `./uninstall.sh` from the extracted archive to remove
runtime files; personal settings and launch history are preserved.

### Build and install on Ubuntu 26.04

Requires **Rust ≥1.93, GTK ≥4.18, libadwaita ≥1.7, Python 3, a C compiler and
pkg-config**. The standard libraries on Ubuntu 24.04 are too old. Other
Linux distributions must meet these versions; they have not been certified here.

```bash
git clone https://github.com/SHADOWOKX/Spotlight-linux.git
cd Spotlight-linux

./scripts/bootstrap-ubuntu.sh
./scripts/install-user.sh
```

if u want to use alt + space disable the default gnome gesture using that command 

```
gsettings set org.gnome.desktop.wm.keybindings activate-window-menu "[]"
```

After installation, open **Spotlight Linux** from GNOME Overview. The default shortcut is **Alt+Space** if available.

## Uninstall

```bash
./scripts/uninstall-user.sh
```

## Build from source

```bash
python3 scripts/build-release.py
```

## Privacy and verification

No telemetry is sent. Optional usage learning stores only application identifiers,
launch counts and last-use times locally. The application data directory is private
(0700); the database and SQLite sidecars are 0600, including migrated older files.
Keep XDG parent directories under your control. Disable learning or clear history
from Settings → Privacy; success is shown only after the database acknowledges it.

```bash
cargo test --workspace --locked  # GTK tests need a display and session bus
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/package-release.py
```

CI runs the GTK suite on an isolated display/bus and tests install/uninstall and
history privacy. See [release validation](docs/release-validation.md). The optional
[GNOME Shell extension](integrations/gnome/README.md) is separate and targets GNOME 50 only.

## License

See [LICENSE](LICENSE).
