# Performance baseline

Recorded on 2026-09-05. These measurements establish a regression baseline; they
are not a substitute for end-to-end profiling in a real GNOME session.

## Host and method

- Intel Core i5-9400F, 6 physical cores, 15 GiB RAM
- Ubuntu 26.04.1, Linux 7.0.0-30-generic
- Rust 1.93.1, optimized benchmark profile
- Criterion warm-up: 3 seconds; 100 samples; 6 searches per sample
- Synthetic immutable catalog: 2,000 desktop applications
- Query: `vst`; requested results: 8
- The measured operation includes fuzzy matching, field weighting, ranking, stable
  sorting, and result construction. It excludes catalog discovery and GTK drawing.

## Result

| Path | p50 | p95 | Criterion mean (95% CI) | Throughput |
| --- | ---: | ---: | ---: | ---: |
| Application search, 2,000 entries, top 8 | 8.44 ms | 8.54 ms | 8.45 ms (8.44–8.46 ms) | 236.7k entries/s |

This is below the initial 30 ms common-application-search target on this host, with
a catalog considerably larger than a typical desktop installation. It does not yet
prove warm activation, rendering, memory, or idle-CPU targets.

Reproduce with:

```sh
cargo bench -p spotlight-core --bench application_search
```

The p50 and p95 above are calculated from Criterion's per-iteration sample values.
Keep this workload stable so later runs expose regressions; add separate workloads
instead of silently changing this one.

## UI refinement: measured search improvement

On the same host and unchanged `2000_entries_top_8` workload (2026-09-05), a
fresh baseline was saved before editing, then compared with the final provider:

| Implementation | p50 | p95 | Criterion mean (95% CI) |
| --- | ---: | ---: | ---: |
| Before refinement | 8.449 ms | 8.474 ms | 8.449 ms (8.447–8.452 ms) |
| Prepared fields + score-only matching | 0.263 ms | 0.266 ms | 0.263 ms (0.2628–0.2632 ms) |

Approximately **32× faster**, or **96.9% less search time**. This is provider search
throughput/latency, **not** a claim of 32× faster window opening. Catalog
initialization and GTK/GNOME presentation are excluded in both runs.

Unicode-normalized fields, IDs and lowercase sort keys are now prepared once at
index construction. Query normalization happens once per search. Score-only DP
reuses two scratch rows and rejects non-subsequences early; an exhaustive short
string/Unicode test compares scores against the unchanged highlighting matcher.
Partial selection sorts only the winning result set. Usage ranking, cancellation
and deterministic ordering remain covered by tests.

GTK now keeps reusable row slots (bounded by the 100-result limit), only reloads
an icon when its description changes, and uses a fixed visible-list viewport to
avoid size jumps while typing. A regression test performs 100 list updates without
allocating another row. Advanced Settings and `--diagnostics` report row counts
and widget-update timing separately from provider search and submitted-paint time.

```sh
# Before the implementation change:
cargo bench -p spotlight-core --bench application_search -- --save-baseline before-ui-refresh
# After the change, same workload:
cargo bench -p spotlight-core --bench application_search -- --baseline before-ui-refresh
```

Percentiles use nearest-rank samples from Criterion's per-iteration times (µs).

## Native renderer comparison on this NVIDIA host

After the identical installed lifecycle workload (20 show/hide cycles and five
Settings openings), a five-second `pidstat` sample of the hidden resident found:

| GTK renderer | Hidden CPU average (one core) | RSS | Proportional memory (PSS) |
| --- | ---: | ---: | ---: |
| GTK default / Vulkan | 1.20% | 199.0 MiB | 89.8 MiB |
| OpenGL | 0.00% measured | 242.2 MiB | 134.5 MiB |

The Vulkan activity appeared in NVIDIA `[vkps] Update` and another driver thread;
GTK's main thread, search, history, and portal threads measured 0%. OpenGL removed
that measured idle activity at the cost of greater memory use on this driver.
This is a short host-specific sample, not a universal or long-duration idle claim.

OpenGL is now the configurable application default. Advanced Settings also offers
GTK Default, Vulkan and Software (Cairo), with restart explicitly required.
An externally supplied `GSK_RENDERER` takes precedence. Startup re-execs once with
the selected environment before GTK initialization; the warmed shortcut path
does not spawn or exec anything. Spotlight's override is stripped from desktop
application and GNOME Settings launch contexts, so it cannot change their renderer.
No driver/system setting is written. GTK's supported selector is documented at
https://docs.gtk.org/gtk4/running.html#gsk-renderer.
