#!/usr/bin/env sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
cargo build --manifest-path "$project_root/Cargo.toml" --target-dir "$project_root/target" --locked --release -p spotlight-gtk

"$project_root/target/release/spotlight-linux" --install-user

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data_root/applications"
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache --force --ignore-theme-index "$data_root/icons/hicolor"
fi

printf '%s\n' \
    "Installed Spotlight Linux for the current user." \
    "Open GNOME Overview and search for Spotlight Linux." \
    "In Settings → Keyboard, confirm Alt+Space or choose an unreserved shortcut." \
    "Enable Launch at Login to keep the shortcut ready after signing in."
