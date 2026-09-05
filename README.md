# Spotlight Linux

Spotlight Linux is a native, keyboard-first launcher for GNOME and Wayland. It is
implemented in Rust with GTK4 and libadwaita, keeps search work off the GTK main
thread, and stores all ranking data locally.

The project is currently in Phase 1. The first vertical slice covers a launcher
window, indexed desktop applications, fuzzy and usage-aware ranking, safe
application launching, appearance preferences, launch-at-login, the Global
Shortcuts portal, diagnostics, tests, and latency instrumentation.

## Repository layout

- `crates/spotlight-core`: UI-independent search, ranking, providers, settings,
  history, and performance instrumentation.
- `crates/spotlight-gtk`: GTK4/libadwaita application and Linux platform adapters.
- `data`: desktop integration and packaged resources.
- `docs`: architecture, delivery plan, risks, and manual test procedures.

## Ubuntu development setup

```sh
./scripts/bootstrap-ubuntu.sh
cargo test -p spotlight-core
cargo run -p spotlight-gtk
```

The setup script prints and runs the minimal Ubuntu package installation command.
It never installs or runs the launcher as root.

The Global Shortcuts portal requires an installed desktop application identity.
For a complete user-local run, including the Wayland shortcut, use:

```sh
./scripts/install-user.sh
```

Then close the terminal. Press Super, search for **Spotlight Linux**, and open it
from GNOME Overview. The palette stays resident when dismissed; Escape, clicking
outside, or closing the window hides it without ending the shortcut session.
Launching the application again focuses the same instance.
The global shortcut toggles the existing palette: press once to open, again to
hide. Escape also hides it without quitting. In Settings → Search, turn off
Show Suggestions for a search-bar-only empty state; typing reveals results.
Ctrl+, still opens Settings when the results and footer are hidden.

Open Settings with **Ctrl+, → Keyboard**. **Alt+Space is the preferred default**.
GNOME often reserves it for Window Menu: Spotlight detects known GNOME bindings,
shows the conflict, and never changes them. Choose an unreserved combination with
**Record Shortcut → Save & Configure**, then confirm it in the native desktop UI.
**Reset to Alt+Space** follows the same confirmation flow.

The desktop owns the final binding. Settings displays both the preference and the
actual binding reported by the portal. Version 2 portals open their configuration
dialog. On GNOME with a version 1 portal, the button opens **GNOME Settings →
Apps → Spotlight Linux → Global Shortcuts**; record the desired key
there too. Changes are received live, without restarting Spotlight. If permission
was declined or the portal failed, **Configure / Retry** remains available.
Conflict diagnostics cover installed GNOME fixed keybindings; other applications
and compositor reservations may still require choosing a different key in the
desktop confirmation UI.

Enable **Settings → General → Launch at Login** to keep the shortcut ready after
each sign-in. This is optional and off by default. **Quit Spotlight** intentionally
ends the resident process; reopen the application icon to restore it. No tray,
terminal, root access, or continually polling helper is required.

This installs only under `$XDG_DATA_HOME` (or `~/.local/share`) and
`$XDG_BIN_HOME` (or `~/.local/bin`); optional autostart is under `$XDG_CONFIG_HOME`
(or `~/.config`). It does not use `sudo`. The desktop entry and D-Bus service both
contain absolute executable paths, so the session does not need `~/.local/bin` in
PATH. GTK, desktop, D-Bus, AppStream, icon, and portal identity all use
`io.github.shadowokx.SpotlightLinux`.

To uninstall:

```sh
./scripts/uninstall-user.sh
```

This stops the resident instance and removes only unchanged tracked runtime files
plus Spotlight-owned autostart. Modified files are reported and preserved. Personal
settings and usage history remain in the standard XDG `spotlight-linux` directories.

## Verification and diagnostics

### Appearance and search customization

Open the visible **Settings** button or press **Ctrl+,**. Appearance includes
system/light/dark, Normal/Glass/Minimal, glass opacity, width, Graphite/System/
Blue/Violet/Green accents, density, row height, icon size, corners, visible-result
count, descriptions and motion. Graphite is the new neutral default. Existing
explicit choices and shortcut bindings survive upgrades. System reduced motion
overrides animation preferences. Glass is real translucency, not simulated blur.

Search settings control application search, maximum results and optional timing
in the footer. Disabling applications cancels in-flight results immediately.
Usage learning can be enabled/disabled and history cleared without a restart;
history I/O stays on its event-driven worker. These are working Phase 1 controls,
not placeholder switches for future providers.

Advanced → Renderer offers OpenGL (default), GTK Default, Vulkan and Software.
Quit/reopen to apply it. An explicit `GSK_RENDERER` environment variable wins over
this setting. The default reduces measured hidden NVIDIA driver activity on the
development host, but uses more memory there; see the comparison below. This
override is local to Spotlight and is removed from contexts used to launch apps.

The current search benchmark is about 32× faster on the unchanged 2,000-entry
workload; see [measurements and limits](docs/performance-baseline.md).

### Checks

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
desktop-file-validate data/io.github.shadowokx.SpotlightLinux.desktop
appstreamcli validate --no-net data/io.github.shadowokx.SpotlightLinux.metainfo.xml
```

GTK tests require a display and session bus. For isolated tests, use
`dbus-run-session --config-file=tests/session-bus.conf -- xvfb-run -a cargo test --workspace --locked` (Xvfb is a test
dependency, not the launcher’s shortcut implementation). The installed binary’s
`--diagnostics` reports the actual resident PID, application ID, window count,
mapped/active/search-focus state, portal connection/session, and current shortcut.
Temporary activation diagnostics keep only the last 256 shortcut/focus events in
memory, including token presence but never token contents or query text.
`--background`, `--toggle`, `--hide`, `--settings`, and `--quit` route to the same
single GApplication instance. Standard D-Bus activation is also installed.

See the [GNOME Wayland acceptance checklist](docs/manual-test-plan.md). Physical
shortcut capture, compositor focus/placement, and login/logout must be checked on
the real desktop before declaring Phase 1 complete. Headless GTK tests alone do
not establish that acceptance.

With the installed app running, `python3 tests/lifecycle-smoke.py
/absolute/path/to/spotlight-linux` exercises repeated and concurrent activation,
hide, and Settings commands. It leaves one resident launcher hidden and does not
send physical or simulated key events.

Set `SPOTLIGHT_SCREENSHOTS` to an existing temporary directory when running the
GTK test to export actual light/dark/glass and Settings widget snapshots. These
use clearly defined test application fixtures on the private display, not a live
desktop screenshot. Do not confuse them with physical GNOME acceptance.

## Principles

- Local providers return independently; one failed provider cannot take down search.
- Every query has a generation token, so obsolete work cannot replace fresh results.
- Desktop applications are launched through `Gio::DesktopAppInfo`; `Exec=` is never
  passed to a shell.
- Global shortcuts use the XDG Desktop Portal on Wayland.
- Configuration follows the XDG base-directory specification. Usage data lives in
  SQLite and no telemetry is collected.
- Blur is optional. Glass mode remains readable without compositor blur.

See the [Phase 1 plan](docs/phase-1-plan.md),
[architecture](docs/architecture.md), and
[performance baseline](docs/performance-baseline.md) for the current scope,
decisions, and measured search latency.
