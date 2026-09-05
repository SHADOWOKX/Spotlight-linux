# Contributing

Thanks for helping improve Spotlight Linux.

## Development target

The current preview target is **Ubuntu 26.04 x86_64 with GNOME/Wayland**. Other distributions are welcome, but they must provide the required GTK/libadwaita versions documented in the README.

Do not run the launcher or its install scripts with `sudo`.

## Setup

```bash
git clone https://github.com/SHADOWOKX/Spotlight-linux.git
cd Spotlight-linux
./scripts/bootstrap-ubuntu.sh
```

## Before opening a pull request

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check-requirements.py
python3 scripts/package-release.py
```

GTK tests need a display and session bus. The CI workflow runs them in an isolated environment.

For launcher behavior, shortcut, focus, login, accessibility, and multi-monitor changes, also follow `docs/manual-test-plan.md` where relevant.

## Pull requests

Keep changes focused. Explain what changed, why it changed, and how it was tested. Include screenshots for visible UI changes when useful.

Do not commit generated `dist/` output, local databases, logs, secrets, `.env` files, or personal build paths.

## Bugs and security issues

Use the bug report template for normal bugs. For security-sensitive issues, follow `SECURITY.md` instead of opening a public issue with exploit details.
