# Phase 1 technical risks

| Risk | Decision / mitigation |
| --- | --- |
| Global Shortcuts portal support varies by desktop and version | Runtime interface/version detection, typed backend status, and a visible Settings explanation. No Wayland-to-X11 hack. |
| Portal binding is user mediated and a preferred trigger is not guaranteed | Request Alt+Space, display the actual portal binding, report known GNOME conflicts without writing system settings, and use native confirmation (GNOME Applications page on v1). |
| A Wayland client cannot force absolute monitor coordinates | Present with the portal activation token and let the compositor place the surface on the active monitor. Validate GNOME behavior manually. |
| Desktop `Exec=` is security-sensitive | Search parses metadata only. Execution is delegated by desktop-file ID to Gio, never to a shell string. Test malicious-looking fields. |
| Application catalogs can contain duplicates or malformed files | Honor XDG path precedence, desktop visibility fields, and isolate parse errors. Keep diagnostics as counts, without dumping arbitrary entry contents. |
| SQLite could add query latency | Load a compact usage snapshot at startup; write after actions on the worker. No database access on each query. |
| Glass/blur behavior differs by compositor | Glass is CSS translucency with a strong contrast floor. Blur is capability-gated and optional; opaque fallback remains first-class. |
| UI dependency versions can outrun distro packages | Declare a conservative GTK/libadwaita API floor and test both native Ubuntu and Flatpak SDK builds. |
| A provider stalls or panics | Cancellation at query generations, provider error isolation, stale-result rejection, and eventually per-provider time budgets. |
