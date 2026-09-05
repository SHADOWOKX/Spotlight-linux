# Phase 1 manual test plan

Run these checks in a normal user session. Never start the launcher with `sudo`.

## GNOME Wayland and global shortcut

1. Confirm `echo $XDG_SESSION_TYPE` prints `wayland`.
2. Run `./scripts/install-user.sh`. Close **every terminal**. Press Super and search
   for **Spotlight Linux**. Verify its original icon and open it from GNOME Overview.
   A bare `cargo run` is not an installation or a portal-identity acceptance test.
3. Open Ctrl+, → Keyboard. Verify **Alt+Space** is the preferred default on a fresh
   configuration. Existing explicit user choices must survive upgrades.
4. If GNOME reserves Alt+Space for Window Menu, verify the conflict message. Do not
   count a returned portal label as proof a reserved key works. Use Record Shortcut
   to choose an unreserved key (for example Ctrl+Alt+Space, if available). Only the
   user may choose to free Alt+Space in GNOME’s system settings.
5. Save and approve the native desktop dialog. On portal v1 GNOME, use the opened
   Apps → Spotlight Linux → Global Shortcuts page to record the same
   combination. Confirm **Current shortcut** updates live to the desktop-approved
   binding. Cancel once and verify no false success; Configure / Retry must work.
6. Close Settings and focus another application, including one with Alt menu
   mnemonics. With Alt+Space approved and Window Menu unbound, perform this cycle
   **10 times slowly, then 10 times rapidly**: a single Alt+Space shows Spotlight
   once and accepts typing immediately → Escape hides it → the next single
   Alt+Space shows it once. A retry is a failure, even if a later press works.
7. Press Alt+Space again while Spotlight is visible: it must hide once; the next
   single press must reopen and focus search. Repeat slowly and rapidly 10 times.
   Confirm no key-release toggle, duplicate windows, crashes, or stale queries. Deactivated is
   diagnostic only; an absent release must not discard the next activation.
   Clicking outside must dismiss the palette; focus moving into Settings or its
   native confirmation dialog must not dismiss Settings.
   Turn off Settings → Search → Show Suggestions: an empty or whitespace query
   must show only the search bar. Typing reveals results; clearing collapses the
   window again, including when a search is still pending. Ctrl+, opens Settings.
   The empty search bar must stay at the expanded panel's top edge. Typing and
   clearing must not move the entry: results appear below it. Transparent margins below the empty bar
   must pass clicks through to the underlying application. In Settings toggle
   Show Suggestions repeatedly: Settings must stay in place with its title bar
   and close button accessible. Repeat on each monitor and with font scaling.
   Drag Settings to either side of the screen: the launcher must not move with
   it. Settings is an independent nonmodal window, not a transient of the palette.
   Change colors and font sizes while both are visible; preview updates remain
   live. Reopening Settings must raise the same window without reattaching it.
8. Reopen Spotlight repeatedly from Overview while visible and while hidden. It
   must focus the existing window; it must never create another resident process.
9. Record a second unreserved key and Save. Confirm it works immediately after the
   explicit desktop confirmation, and the previous key no longer summons Spotlight.
10. Reset to Alt+Space. Repeat the conflict/approval check; do not forcibly change
    GNOME settings. Verify the preference survives reopening and login.
11. On a disposable test session, stop/restart the portal. Local search must continue,
    Settings must explain the interruption, and a fresh identity/session must be
    established on the portal’s return. Declining authorization must leave retry usable.
12. Enable Launch at Login, log out/in, and summon without opening a terminal or
    app icon. Verify it starts hidden. Disable the switch and verify only Spotlight’s
    owned autostart entry is removed. Quit Spotlight intentionally ends the session;
    reopening the icon restores it.

### Temporary activation diagnostics

`spotlight-linux --diagnostics` retrieves the resident's last 256 activation/focus
events from memory. It records portal Activated/Deactivated timestamps, token
presence (never the token), present requests, mapped/unmapped state, window
activation, search-widget focus, first submitted paint, and dismissal reasons.
It collects no ordinary keys, query text, or clipboard data; there is no polling
or persistent event file. `activations`, `forwarded`, and `shortcut_requests`
should agree after delivery. Every Activated must lead to one show/focus request;
Deactivated must lead to none. Check for an unexpected `focus-loss`/`unmapped`
before Escape, or a present request never followed by `active=true`.

Search-widget focus can remain logically true in a hidden window. Actual typing
requires the window to be visible, mapped, and active as well. Submitted-paint
timing is not proof of compositor presentation or end-to-end physical key latency.

## Installed assets and uninstall

1. Verify these paths (or the equivalent configured XDG locations):
   `~/.local/bin/spotlight-linux`,
   `~/.local/share/applications/io.github.shadowokx.SpotlightLinux.desktop`,
   `~/.local/share/dbus-1/services/io.github.shadowokx.SpotlightLinux.service`,
   `~/.local/share/icons/hicolor/scalable/apps/io.github.shadowokx.SpotlightLinux.svg`,
   `~/.local/share/metainfo/io.github.shadowokx.SpotlightLinux.metainfo.xml`.
2. Desktop `Exec` and D-Bus service `Exec` must use an absolute executable, with no
   shell. Desktop `DBusActivatable=true`; service name, GTK ID, and portal identity
   must match `io.github.shadowokx.SpotlightLinux`.
3. Run `~/.local/bin/spotlight-linux --diagnostics` before and after 20 `--toggle`
   invocations. PID and window count must stay constant (one resident, one window).
   This is a lifecycle test, **not** a substitute for physical shortcut testing.
4. Run `./scripts/uninstall-user.sh`. Confirm the process stops, all unchanged
   installed assets above disappear, and owned autostart is removed. Unrelated
   files, GNOME bindings, and personal config/history must remain. Repeat a fresh
   install and launch from Overview. Also test staged installation with a binary
   directory containing spaces.

## Keyboard flow

1. Search for an installed application without touching the mouse.
2. Navigate with Up/Down and Ctrl+J/Ctrl+K.
3. Press Tab and Ctrl+Enter to open the action menu; Escape closes it first.
4. Press Enter to launch. Confirm the launcher closes according to Settings.
5. Press Ctrl+, to open Settings and verify focus/keyboard navigation.

## Multi-monitor placement

1. Test with monitors on both sides of the primary display and at mixed scale factors.
2. Focus a window on each monitor, invoke the portal shortcut, and confirm GNOME
   presents the launcher on the active monitor without a jump from another monitor.
3. Repeat while moving workspaces and while one monitor is fullscreen.

## Appearance and accessibility

0. Open the visible Settings button or Ctrl+,. Change accent, density, row/icon
   size, corners, list height and descriptions. Verify immediate updates and
   persistence after a restart. Drag Glass Opacity, immediately close Settings,
   and verify the final value is saved. Settings must retain all unrelated choices.
1. Test Follow System, Light, and Dark themes with Normal, Glass, and Minimal styles.
2. Confirm every selected row remains readable at the minimum glass opacity.
3. Enable GNOME reduced motion, then test Reduced and Off animation preferences.
4. Increase system font scaling to 1.5× and 2×; labels may ellipsize but must not
   overlap or hide the selected state.
5. Inspect the window with Accerciser: the search entry, rows, and buttons need useful
   names and keyboard focus.
6. Disable Applications in Search while typing; stale rows must disappear and
   actions become disabled. Re-enable it without restarting and check results.
   Turn usage learning off, launch an app, and verify no new history is recorded;
   turn it on again and verify recording resumes. Clear history while learning is
   off and verify it is actually cleared when re-enabled.

## Reliability, performance, and idle behavior

1. Add malformed and hidden `.desktop` fixtures under a temporary XDG data directory.
   Confirm they are skipped and valid applications remain searchable.
2. Type and erase rapidly; stale results must never replace the current query.
3. Inspect Advanced diagnostics after at least 50 searches and record p50/p95.
4. Profile warm shortcut-to-first-frame and search with Sysprof in a release build.
5. Leave the hidden launcher idle for ten minutes. Verify no polling wakeups and
   approximately zero CPU use.
6. Enable launch-at-login, log out/in, and confirm the process starts hidden and the
   shortcut works. Disable it and confirm the autostart file is removed.
