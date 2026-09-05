# Preview release validation

Version: **0.1.1-preview.1**. Target: Ubuntu 26.04, x86_64.
This is a preview, not a claim of completed physical GNOME acceptance.

## Automated evidence

- 42 core tests and 13 GTK tests pass, including legacy history migration while
  learning is disabled, private files, symlink/hardlink rejection, two SQLite
  connections, filename-only compatibility, and history-clear success/failure.
- Formatting and Clippy with warnings denied pass.
- GNOME extension logic tests and Desktop/AppStream validation pass.
- `tests/history-privacy.py` exercises the real store with umask 022 and checks
  database/WAL/SHM 0600 behind an application directory 0700. The CI container
  additionally drops to another UID and verifies filesystem reads are denied.
- `tests/release-smoke.py` verifies the archive checksum and ownership/modes,
  installs the binary into temporary XDG paths with spaces, runs lifecycle
  checks (20 toggle/hide cycles, 20 concurrent background invocations, five
  Settings activations), uninstalls and verifies personal data survives.
- Release builds remap checkout and Cargo/home source paths. Packaging rejects
  remaining home-directory strings. No usage data or personal build paths are
  included in the release archive.

Run the GUI tests only on the private bus/display to avoid affecting your session:

```sh
GTK_A11Y=none GSK_RENDERER=cairo dbus-run-session \
  --config-file=tests/session-bus.conf -- xvfb-run -a cargo test --workspace --locked
python3 tests/history-privacy.py
python3 scripts/package-release.py
GTK_A11Y=none GSK_RENDERER=cairo GSETTINGS_SCHEMA_DIR=/usr/share/glib-2.0/schemas \
  dbus-run-session --config-file=tests/session-bus.conf -- xvfb-run -a \
  python3 tests/release-smoke.py
```

Private-bus portal-unavailable and Xvfb acceleration warnings are expected. They
are not evidence of a working physical portal shortcut.

## Privacy migration

Existing owned history and SQLite sidecars are tightened during installation and
startup, including when learning is disabled. No database is created for a fresh
opted-out user. Unsafe symlinks, hardlinks, nonregular or foreign-owned history
are rejected with an error instead of silently following them. XDG ancestors
remain unchanged and must be under the user's control; hostile same-UID writers
are outside this filesystem confidentiality boundary. Clearing removes usage rows;
it is not a guarantee of forensic erasure from backups or storage hardware.

## Before calling this stable

Complete [manual-test-plan.md](manual-test-plan.md) on GNOME/Wayland: physical
shortcut confirmation, focus/monitor placement, login/logout, and the optional
GNOME 50 extension. Bootstrap provisioning on a clean machine and additional
Linux distributions remain separate acceptance work. Do not describe this
preview as compatible with Ubuntu 24.04's standard GTK libraries.
