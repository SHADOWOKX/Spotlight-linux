#!/usr/bin/env sh
set -eu

binary_root=${XDG_BIN_HOME:-"$HOME/.local/bin"}
data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
case "$data_root" in /*) ;; *) data_root="$HOME/.local/share" ;; esac
if [ ! -x "$binary_root/spotlight-linux" ]; then
    printf '%s\n' "Spotlight Linux is not installed at $binary_root/spotlight-linux" >&2
    exit 1
fi
"$binary_root/spotlight-linux" --uninstall-user
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data_root/applications"
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache --force --ignore-theme-index "$data_root/icons/hicolor"
fi
printf '%s\n' "Removed unchanged Spotlight-owned runtime assets. Your settings and usage history were preserved."
