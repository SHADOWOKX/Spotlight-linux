# Phase 1 desktop integration repair

The current installed desktop file depends on a shell PATH. The running GNOME
portal's PATH does not contain `~/.local/bin`, so Gio rejects that desktop entry
and `Registry.Register` reports that its application ID cannot be found. The app
then ignores that error and attempts an anonymous shortcut session.

This repair has four parts:

1. Install absolute executable paths, a matching D-Bus activation service, and
   ownership-tracked assets. Add an uninstall path and installation tests.
2. Keep one GApplication resident with an explicit hold. Route activation and
   command-line options to that instance; hiding never destroys it. Add explicit
   Quit, focus-loss dismissal, and lifecycle regression tests.
3. Register a dedicated D-Bus connection before any portal operation. Keep failed
   setup retryable; filter signals to the live session; recover from portal
   restarts. Default to Alt+Space, show requested and actual bindings, detect GNOME
   conflicts through read-only settings, and provide a recorder plus the desktop's
   confirmation/configuration flow. Support the host's actual version 1 backend.
4. Run the complete Rust/GTK suite, strict linting and metadata checks, exercise
   real user installation and D-Bus activation, and document remaining physical
   keyboard/multi-monitor checks without claiming they were automated.

The Global Shortcuts portal binds shortcuts to a live client session. An idle,
event-driven resident process is therefore required for immediate warm summons;
D-Bus activation provides reliable terminal-free startup and single-instance
forwarding, not a replacement for that live shortcut session.

Reference: [Registry identity rules](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html),
[Global Shortcuts lifecycle](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html).

## Verification status — in progress

The real installer/uninstaller, native D-Bus startup, installed Gio identity,
Registry.Register and a live portal v1 session have been verified. The Rust suite
(31 core + 7 frontend tests), strict Clippy, formatting and metadata checks passed
before the latest responsiveness instrumentation. Repeated command activation
kept a single resident process/window; Settings uses a separately tracked native
dialog because the palette is non-resizable.

**Physical keyboard acceptance is not passing yet.** The user reports slow,
unreliable Alt+Space response and needing repeated presses. A portal-returned
binding and successful command-line lifecycle tests do not prove that workflow.
Event-only diagnostics are being added to distinguish received/suppressed portal
activations, missing releases/tokens, focus-loss dismissal and time to paint.
No ordinary key events or search text are collected. Phase 1 must remain open
until this is corrected and physically retested.

## First-press activation repair — awaiting physical acceptance

The diagnostic resident received 44 Activated events and 16 Deactivated events;
its release-dependent latch suppressed 27 activations. The user confirmed these
were normal individual presses, with retries only when nothing appeared. The
latch was therefore discarding legitimate activations. Increasing debounce is
not an acceptable solution.

The latch is removed. Every Activated matching the current session and shortcut
is forwarded once. It now shows/focuses the existing window idempotently, never
toggles it closed. Deactivated records a diagnostic event only. Explicit CLI
`--toggle` is separate. Each available token is applied to the target GTK window
before `present()`; a visible palette is still raised and search receives focus.
Only a previously hidden palette selects the remembered query, so extra
activations do not disrupt ongoing typing. Focus-loss dismissal is armed only
after the current presentation gains focus, not by a stale inactive notification.

The bounded trace retains 256 events in memory with no idle work, disk logging,
ordinary keystrokes, query text, or token values. Added regression coverage for
unpaired activations, stale inactive notifications, repeated show/Escape cycles,
idempotent repeated activation and bounded diagnostic storage. The updated suite
passes 31 core and 9 frontend tests on the isolated display/private session bus.
These tests do not validate real compositor activation. The required acceptance
remains 10 slow and 10 rapid physical Alt+Space/Escape cycles from other apps,
with no failed first press. Alt+Space and GNOME system bindings are unchanged.

Token handoff follows [GTK's startup-ID ordering](https://docs.gtk.org/gtk4/method.Window.set_startup_id.html)
and the [portal Activated contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html#org-freedesktop-portal-globalshortcuts-activated).

The optimized repair build was installed for the current user and activated
through the installed D-Bus service (resident PID 248740 during verification).
The installed and built executables have identical SHA-256 digests. The live
portal v1 still reports `Press <Alt>space`, with no registration error. Strict
Clippy, formatting, desktop metadata and AppStream validation passed. The installed
lifecycle check passed 20 toggle/hide cycles, 20 concurrent background commands,
and five repeated Settings activations; it left one resident window hidden.
This used explicit application commands, not the shortcut. The physical test
has been requested and its result is still pending.
