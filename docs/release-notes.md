# Spotlight Linux 0.1.1-preview.1

A preview for Ubuntu 26.04 x86_64, with native application search, calculator and
optional portal shortcuts.

- Protect launch history and SQLite sidecars, including legacy data when learning
  is disabled; preserve records during permission migration.
- Confirm history clearing only after the worker succeeds and report failures.
- Ignore invalid relative XDG roots consistently and preserve user files on uninstall.
- Include the GPL-3.0 license, native requirement checks, CI and a ready-built archive.
- Build release artifacts with neutral source paths and a SHA256SUMS file.

Download the archive and SHA256SUMS, verify the checksum, extract and run
`./install.sh` as your normal user. Rust is not required. Use `./uninstall.sh`
from the extracted directory to remove runtime files while keeping personal data.
The GTK/libadwaita runtime from Ubuntu 26.04 is required.

This is a prerelease. Physical GNOME/Wayland shortcut/focus/login behavior and the
optional GNOME 50 extension still need manual acceptance; isolated GUI tests do
not establish that. See the repository's release-validation document.
