# Native palette refinement

Scope: refine the existing Phase 1 product, not add incomplete providers or
replace GTK. References are Raycast's keyboard-first command surface and Apple's
Spotlight search hierarchy, not their branding/assets:
https://www.raycast.com/ and
https://support.apple.com/guide/mac-help/search-with-spotlight-mchlp1008/mac.

1. Record a fresh, unchanged Criterion workload before optimizing. Prepare
   Unicode search fields/IDs/sort keys once when indexing; use a score-only
   matcher with reusable scratch memory and prove score parity with the existing
   highlighting matcher. Preserve cancellation and ranking semantics.
2. Replace duplicate search decoration, saturated selection, tall rows and
   debug-heavy footer with a restrained native palette, visible Settings and
   Actions buttons, and keyboard hints. Reuse row widgets between searches and
   expose UI-update timing separately from worker search time.
3. Extend native Settings with real live appearance controls and search behavior:
   neutral/system/preset accents, density, row height, icon size, corner radius,
   visible results, motion, subtitles, maximum results and provider enablement.
   Keep existing config compatible. Do not advertise unsupported compositor blur.
4. Test ranking parity, config round trips/bounds, repeated rendering/widget
   reuse, CSS loading and keyboard lifecycle. Render actual GTK light/dark/glass
   and settings snapshots on an isolated display; inspect them before shipping.
   Compare the identical benchmark, build/install for the current user and ask
   for physical Wayland UX feedback.

Risks: GTK/libadwaita CSS specificity can leave native focus borders visible;
   font scaling must not be defeated by fixed total widget heights. Glass is
   translucency only on stock GNOME, never screenshot blur. Worker timings are
   not key-to-compositor latency. Global shortcut acceptance remains unconfirmed;
   its token/release handling must remain intact throughout this refinement.

## Verified implementation

- Search: 8.449 ms → 0.263 ms mean on the identical 2,000-application benchmark;
  approximately 32× faster. See `performance-baseline.md` for method and limits.
- Row widgets are reused across 100 repeated updates; icon descriptions are
  compared before resetting images. The viewport remains stable while typing.
- A single search decoration, restrained selection, application type labels,
  keyboard hints and working Settings/Open/Actions controls replace the old UI.
- Appearance/Search controls work live; legacy configuration has additive defaults.
  Usage history now supports enable/disable/clear on its worker without restarting.
- 33 core + 11 frontend tests and strict Clippy pass. GTK dark/light/glass and
  Settings images were rendered and visually reviewed on the isolated display.
  Glass interior alpha measured 230/255 versus 255/255 for Normal at 90% opacity.
  Xvfb emits expected missing-DRI3 diagnostics; no GTK CSS/markup warning remains.
- Prior resident diagnostics showed 23 Activated events forwarded to 23 UI
  requests, with tokens on every request. No release-dependent suppression remains,
  but user-confirmed physical shortcut latency/monitor acceptance is still open.

The release was installed through `scripts/install-user.sh`, then started through
the native D-Bus service. Installed and built SHA-256 hashes match. The resident
reported 51 indexed apps, eight warmed row widgets and the existing portal v1
`Press <Alt>space` binding, with no shortcut error. Physical typing/appearance
feedback on the user's GNOME desktop remains the final acceptance step.

An additional idle measurement found driver-owned Vulkan wakeups on the native
NVIDIA session. A like-for-like OpenGL comparison measured 0% instead of 1.2%
CPU over five seconds, with higher memory usage. Added an explicit Advanced
renderer preference, defaulting to OpenGL, with user/environment overrides,
restart semantics and child-environment isolation. See the performance document
for the memory tradeoff; neither renderer result substitutes for physical input
latency testing.

The final native-service resident reported `GskGLRenderer`, one window, 51 apps,
eight warmed rows, and the unchanged Alt+Space portal binding. The full installed
lifecycle check passed again. The temporary renderer-comparison service was
collected (`not-found`/`inactive`); no persistent diagnostic service remains.
