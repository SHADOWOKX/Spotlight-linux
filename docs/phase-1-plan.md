# Phase 1 implementation plan

Each increment must leave a testable product; later increments do not block the
launcher from opening and searching applications.

## 1. Core search vertical slice

- Define typed result/action/provider contracts and generation cancellation.
- Parse and deduplicate XDG desktop applications.
- Implement fuzzy, usage, and recency ranking with deterministic ordering.
- Add a latest-query worker and latency snapshots.
- Add unit tests and a criterion benchmark for the application search path.

Exit criteria: core tests pass; a warm search over a synthetic 2,000-application
catalog has a recorded p50/p95 baseline and stale queries cannot emit accepted
results.

## 2. Native launcher

- Build the undecorated GTK4/libadwaita window and compact result rows.
- Make the entry focused immediately and keep keyboard handling complete.
- Connect query updates to the worker; execute application actions with Gio.
- Apply Normal and Glass styles with readable light/dark/system variants.

Exit criteria: launcher is usable without a mouse, no query work happens on the GTK
thread, and a malformed desktop entry cannot execute through a shell.

## 3. Desktop integration

- Establish and restore an XDG Global Shortcuts portal session.
- Surface portal permission/unsupported/rejected states in Settings.
- Add launch-at-login through an XDG autostart desktop file.
- Present on portal activation using the compositor activation token where exposed.

Exit criteria: Alt+Space (or an explicitly approved unreserved replacement) activates on GNOME Wayland, a changed
binding is configurable, and unsupported desktops receive actionable guidance.

## 4. Preferences, persistence, and diagnostics

- Implement General, Appearance, Keyboard, Privacy, Advanced, and About pages for
  the Phase 1 controls only.
- Persist versioned TOML configuration atomically.
- Persist launches to SQLite and refresh the in-memory ranking snapshot.
- Show environment, portal backend, catalog size, and search latency diagnostics.

Exit criteria: settings survive restart, history improves ranking, clearing local
usage works, and diagnostics never contain queries or clipboard data.

## 5. Polish and packaging readiness

- Add app metadata, icons, metainfo, desktop file, and Flatpak manifest skeleton.
- Run accessibility inspection and the full manual test matrix.
- Profile cold start, warm activation, typing, idle CPU, and memory before tuning.
- Fix warnings and document measured baselines; do not claim unmet targets.

Exit criteria: the Phase 1 quality checklist and manual matrix pass on Ubuntu GNOME
Wayland, including multiple monitors and reduced motion.
