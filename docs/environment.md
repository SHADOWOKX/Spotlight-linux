# Environment survey

Surveyed on 2026-09-05 before implementation.

## Host

- Ubuntu 26.04.1 LTS
- GNOME Shell 50.1
- Wayland session (`XDG_SESSION_TYPE=wayland`)
- GTK 4.22.4 runtime and libadwaita 1.9.1 runtime
- xdg-desktop-portal 1.21.1 with xdg-desktop-portal-gnome 50.0

The installed GNOME portal descriptor advertises
`org.freedesktop.impl.portal.GlobalShortcuts`. A subsequent live D-Bus check found
**GlobalShortcuts version 1** in both frontend and GNOME backend, despite newer
package versions and a ConfigureShortcuts method in introspection. The client
must check the actual version property and offer the GNOME Settings fallback.

## Missing development prerequisites

The initial machine image did not contain `rustc`, `cargo`, `libgtk-4-dev`, or
`libadwaita-1-dev`. Runtime libraries are present. `scripts/bootstrap-ubuntu.sh`
installs the required build packages. The main program must never be run with
`sudo`.

The sandbox used for the initial inspection could not connect to the user's
session D-Bus. A later, explicitly approved desktop smoke run kept the launcher
alive and indexed applications successfully. As expected for an uninstalled
development binary, the portal rejected shortcut registration. The later user
installation still failed: its bare `Exec=spotlight-linux` could not resolve with
the running portal's PATH, which excluded `~/.local/bin`. Gio rejected the desktop
entry, and Registry.Register returned “App info not found”. The integration repair
installs absolute executable paths and treats registration failure as actionable.
GNOME's `activate-window-menu` is currently `<Alt>space`; it has not been changed.
Physical shortcut activation and multi-monitor placement remain acceptance tests.

During the repair, system Rust/Cargo 1.93.1, GTK 4.22.4 development headers and
libadwaita 1.9.1 development headers were available. Xvfb is used only for isolated
GTK tests, not for launcher integration.

## Consequences

- Target GTK 4.18 and libadwaita 1.7 APIs even though this host is newer, keeping a
  practical packaging floor for recent Ubuntu releases.
- Use the Global Shortcuts portal first. Detect absence or rejection and expose a
  useful status in Settings/Diagnostics; never fall back to an X11-only grab on a
  Wayland session.
- Keep the launcher as one process in Phase 1. Application metadata is loaded once
  and searched in memory. No permanently busy daemon is justified yet.
