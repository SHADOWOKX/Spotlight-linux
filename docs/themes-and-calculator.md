# Themes and local calculator — incremental delivery

Keep the established resident process, portal activation, GNOME companion and
fixed search-entry coordinates unchanged. Build on the existing reusable GTK
rows, typed actions and cancellable worker pipeline, without new dependencies.

This increment adds Native / Graphite / Midnight / Dusk / Forest color palettes.
Each follows the selected System / Light / Dark mode and works with Normal,
Glass or Minimal. Existing accent, density, radius, icon size and opacity controls
remain independent. New bounded search/result font sizes respect system scaling;
result-type labels can be hidden. Existing configurations keep Native appearance.

Calculator is a separate instant provider. Examples: `2+2`, `sqrt(144)`,
`15% of 850`, `(125+8)*4`, `2^3`, `abs(-4)` and `round(1.6)`.
Enter copies the result using the native clipboard. Settings → Search can disable
it independently of applications. No shell execution or network requests.
Input is limited to 256 ASCII bytes and 32 recursive levels; incomplete syntax,
division by zero and non-finite values produce no calculator result.
Numbers use binary floating point, not accounting-grade decimal arithmetic.
Conversions, file indexing, clipboard history and quicklinks are separate future
increments, not represented as completed functionality here.

Verification: run core and GTK tests, Clippy, the application-search benchmark,
and render dark/light palette fixtures. Manually confirm Alt+Space, fixed entry
position, settings geometry, typing and clipboard copy on GNOME Wayland.

Measured on the development machine: 2,000-app top-8 benchmark 252–254µs;
percentage parser 74ns (10 samples, one-second warmup/measurement). These are
microbenchmarks, not end-to-end display latency. 47 automated tests passed,
including calculator-only search, disabling providers, config round-trip and
stable palette/Settings geometry. Dark/light fixtures rendered for every palette.
