# Architecture

## Process model

Phase 1 is a single long-lived, event-driven process. GTK owns the main thread.
Application discovery and query evaluation run on a dedicated worker, fed by a
bounded latest-query channel. The process does no periodic polling and is idle when
the UI and filesystem are idle.

```text
Global Shortcuts portal ──activation──> GTK application
                                           │
Search entry ──query generation────────────┼──> search coordinator
                                           │         │
                                           │         ├─ application provider
                                           │         ├─ future instant providers
                                           │         └─ future streaming providers
                                           │
GTK result model <──generation-tagged update─────────┘
       │
       └── primary action ──> Linux action adapter ──> Gio DesktopAppInfo
```

The result update carries its query generation. GTK discards stale generations,
which provides a second cancellation boundary even if a provider fails to observe
its cancellation token promptly.

## Crates

### `spotlight-core`

No GTK dependency. It owns stable domain concepts:

- `model`: provider IDs, results, icons, actions, and query generations.
- `ranking`: fuzzy matching plus prefix, word-boundary, usage, and recency boosts.
- `provider`: failure-isolated, cancellation-aware provider contract.
- `providers::applications`: immutable in-memory desktop application catalog.
- `desktop_entry`: desktop entry parsing and filtering. Launching is deliberately
  outside this module.
- `history`: local SQLite usage counts and timestamps.
- `settings`: versioned TOML settings and XDG paths.
- `search`: latest-query coordinator and incremental result batches.
- `performance`: monotonic spans and structured latency snapshots.

### `spotlight-gtk`

Owns presentation and Linux APIs:

- `app` and `ui`: application lifecycle, launcher, result rows, preferences.
- `actions`: translates typed core actions to Gio calls; no shell parsing.
- `platform::global_shortcut`: XDG Global Shortcuts portal session.
- `platform::autostart`: explicit XDG autostart desktop file management.
- `style`: Normal and Glass CSS with opaque fallback and system color schemes.

## Search contract

Providers receive an immutable query, a generation-scoped cancellation token, and a
result sink. They return typed errors. The coordinator catches provider failures,
records diagnostics, and continues with remaining providers. Provider metadata says
whether a provider is instant or delayed; only delayed providers may be debounced.

Phase 1 has one instant provider. Its catalog is built once at startup and refreshed
only in response to application-directory filesystem events in a later increment.
Query-time work is bounded by the number of desktop entries and allocates only the
top result candidates.

The index stores normalized Unicode search fields, stable result IDs and sort
keys. Queries use reusable score-only DP rows, then select/sort the top candidates.
The GTK layer reuses result-row widget slots across generations; a row's icon is
only reset when its icon description changes. UI-update measurements remain
separate from worker and compositor timings. The history worker accepts explicit
enable/disable/clear commands, so privacy changes do not require a restart.

## Ranking

The base fuzzy score rewards, in descending order: exact match, exact prefix,
token-prefix/word boundaries, consecutive characters, and compact matches. Usage
adds a logarithmic launch-count boost and a decaying recency boost. Provider
priority is a small final term so it cannot rescue a poor textual match.

Stable tie-breakers are score, case-folded title, then result ID. This prevents rows
from jumping between identical searches.

## Storage

```text
$XDG_CONFIG_HOME/spotlight-linux/config.toml
$XDG_CACHE_HOME/spotlight-linux/
$XDG_DATA_HOME/spotlight-linux/history.sqlite3
$XDG_CONFIG_HOME/autostart/io.github.shadowokx.SpotlightLinux.desktop
```

Configuration writes use a same-directory temporary file followed by rename.
History uses WAL-mode SQLite with short transactions. Search reads use an in-memory
usage snapshot, so SQLite never runs per keystroke.

## Window placement

Wayland intentionally prevents clients from choosing arbitrary global coordinates.
On portal activation the application presents its existing resident window using
the activation token when available. Placement and focus remain compositor-owned;
GTK cannot guarantee centered active-monitor placement on stock GNOME Wayland.
The compositor's behavior must be verified on the target monitors; this is an
explicit acceptance limitation, not a claim that GTK controls global coordinates.

## Resident lifecycle and installed identity

A single GApplication owns one explicit hold and one warmed launcher window.
Close/Escape/focus loss hide the window, not the process. The shortcut session
stays live until explicit Quit or desktop-session teardown. Command lines and
D-Bus actions forward to that instance; no auxiliary polling daemon is used.

The user installer generates absolute Exec paths in matching desktop and D-Bus
service files. A dedicated zbus connection registers the reverse-DNS host AppID
before any portal call. Its shortcuts are filtered by session path and shortcut
ID; key repeats cannot toggle repeatedly while the trigger is held. Portal owner
changes drive reconnection with fresh registered connections. Setup failure leaves
configuration/retry reachable. Preference, actual desktop binding, and conflict
diagnostics are deliberately separate. See [the repair plan](desktop-integration-plan.md).

## Extension boundary

No extension runtime is built in Phase 1. Provider/action interfaces avoid GTK
types, which leaves room for a later out-of-process protocol. Native third-party
libraries will not be loaded into the launcher process by default.
