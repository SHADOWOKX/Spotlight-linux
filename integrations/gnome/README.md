# Optional GNOME 50 integration

This companion extension hides only the window with application ID
`io.github.shadowokx.SpotlightLinux` and title `Spotlight Linux` from window lists.
It bypasses GNOME's window animation decision for that palette. Settings windows,
other applications, portal shortcuts, and global animation settings are unchanged.
There are no timers, network calls, shell commands, or key interception.

The extension uses GNOME 50's `Meta.Window.hide_from_window_list()` and the private
`Main.wm._shouldAnimateActor` hook; it is deliberately not advertised as compatible
with untested GNOME releases. Other animation extensions may override that hook.
Disable restores only visibility changed by this extension, disconnects listeners,
and removes its hook without overwriting a later extension's replacement.

Run from the repository root:

```sh
gjs -m tests/gnome-integration.js
gnome-extensions pack integrations/gnome/spotlight@shadowokx --extra-source=controller.js --out-dir=/tmp --force
gnome-extensions install /tmp/spotlight@shadowokx.shell-extension.zip
```

For first installation GNOME may require logout/login before it discovers the
extension. Save work first; do not restart GNOME Shell on Wayland. Then run:

```sh
gnome-extensions enable spotlight@shadowokx
gnome-extensions info spotlight@shadowokx
```

Rollback: `gnome-extensions disable spotlight@shadowokx`. The launcher remains
fully functional without this companion. Uninstall separately using
`gnome-extensions uninstall spotlight@shadowokx`; the native app installer does
not implicitly install or remove shell extensions.

Manual acceptance: Alt+Space opens the palette without a running dock icon or
GNOME scale/fade effect. Escape and Alt+Space hide it. Repeat 10 slow and 10 rapid
cycles; entry focus must remain immediate. Open Settings: it remains a normal
window. Check another app's icon/animations are unchanged. Disable while the
palette is open: its window-list entry returns. Re-enable and repeat. A manually
pinned favorite may still be shown in the dock; favorites are never edited here.

Automated tests cover matching, late identity, settings/other-app exclusion,
previously-hidden windows, signal cleanup, and restoration. These are not proof
of compositor animation smoothness; confirm that in the real GNOME session.
