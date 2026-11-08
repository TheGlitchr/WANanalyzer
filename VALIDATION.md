# Development build validation — November 8, 2026

## Automated checks

- `cargo fmt --all -- --check` passed.
- `cargo clippy --locked --target x86_64-pc-windows-gnullvm --all-targets -- -D warnings` passed.
- `cargo test --locked --target x86_64-pc-windows-gnullvm` passed: 26 tests (16 library and 10 integration tests), including real Windows loopback ICMP, per-hop loss accounting, provider fixture parsing, fresh/stable result gating, legacy record migration, all-provider restart/export persistence, SQLite recovery, and export safety.
- New regression coverage checks current-navigation loading, stale browser callbacks, queued measurements appearing in exports, protection of the active database and SQLite sidecars, atomic settings replacement, and preserving existing outputs when export/replacement fails.
- Optimized Windows x64 portable EXE built with the project-local LLVM-MinGW toolchain.
- PE import inspection found only Windows/system libraries; no separately distributed SQLite, GCC, or WebView2 loader DLL is required beside the EXE. The embedded WebView2 loader is extracted on demand, and an existing WebView2 Runtime is required for the speed-test backends.

## Earlier desktop and provider checks

- The custom Signal Orbit icon was inspected at 16, 24, 32, 48, 64, 128, and 256 px. The release PE contains seven ICON resources and one GROUP_ICON resource; Windows' associated-icon extraction returned the custom artwork. The window icon is compiled from embedded PNG bytes, so neither icon needs a distributed sidecar file. Formatting and Clippy checks passed after integration.
- The Monitor view displayed ten real traceroute hops and all table columns together on the available 1024 × 768 display. Both compact charts remained visible beneath the table. Longer traces scroll within the table.
- A normal launch of the final EXE opened maximized after the first-frame sizing request; its title bar exposed Restore and the complete layout fit the work area.
- The genuine WiFiman document rendered as a native child pane inside the main window's Speed tests panel. Window inventory showed a single WANanalyzer top-level window throughout the test.
- A genuine embedded test automatically completed and committed 796.41 Mbps download, 777.61 Mbps upload, and 21 ms provider ping. The result appeared in the local panel and remained present after restarting the portable EXE. Native probing continued throughout.
- Existing preview history was preserved using SQLite's consistent backup operation before moving to `dist/data`; the original database remains in `artifacts/ui-smoke`.
- The four-provider revision displayed all quadrants with individual Load and Expand controls. Existing logged measurements rendered with plain Download/Upload/Ping labels, and recording status used ASCII text without missing-glyph boxes.
- Devina's genuine bufferbloat page loaded inside its lower-right native child panel.
- Speedtest.net and FAST.com loaded as native child documents beside Devina. FAST completed its automatic download test and exposed its completed-result controls. These provider results are displayed interactively, not imported into the WiFiman log.
- Expand switched Speedtest.net to a larger main-window panel; Grid returned to the four provider controls. Accessibility state confirmed both transitions. F6 returned focus to the main application region.
- A WiFiman run initiated during the four-provider checks was canceled and persisted as canceled with null throughput values; existing successful results remained intact. The completed WiFiman capture described above was verified before this revision.

## Polishing pass checks

- Reviewed the navy/mint app shell, target states, recording footer, ten-hop table, and both charts on the available 1024 × 768 desktop at 90% scale. Monitor metrics now use adaptive 6/3/2-column rows; a vertically scrolling compact view handles limited height.
- Inspected centered Settings at 135% scale. Changing its draft left the current UI unchanged; Escape discarded the draft. A subsequent change to 90% applied, closed the dialog, and persisted the exact settings to the isolated preview directory. Backward keyboard navigation stayed inside the modal.
- F1 opened the centered About dialog and focused Close. Its backdrop hid the native provider pane; Escape restored the pane. Ctrl+1/2/3 and Ctrl+, worked with app keyboard focus.
- Devina progressed from Loading to Open only after navigation completed, rendered inside its quadrant, and expanded inside the same main window at the retained 25% zoom. Saved-only filtering removed canceled entries visually, and hiding the log expanded the browser to the available width.
- The final build preserved keyboard focus when Expand became Back to grid and when returning to the grid. A fresh WiFiman test automatically saved 577.5 Mbps download, 755.9 Mbps upload, and 23 ms ping while native monitoring continued.
- A background all-history snapshot completed from an isolated copy of existing history while the UI displayed progress and disabled duplicate export actions. The resulting 218,497,024-byte database passed SQLite `quick_check`, retained 408,760 probes and seven speed records, and remained readable after normal app closure.
- Existing `dist/data` was not used for these polish tests. The preview used a consistent SQLite backup under `artifacts/polish-preview/data`.
- Previous polish-build EXE: 15,072,768 bytes, SHA256 `795DC32657C871E6D9F46CADE5DFE42BB54F2678F49925EFD9F3A4ACEEC1FE21`. Its PE inspection retained the icon/group-icon resources and imports only Windows/system libraries.

## Native provider-card revision

Four sequential tests completed in hidden WebView2 backends using an isolated `artifacts/native-speed-preview/data` directory. Each produced exactly one durable completed record. These are functional development checks, not comparable performance baselines.

| Provider | Download Mbps | Upload Mbps | Ping / idle ms | Download-loaded ms | Upload-loaded ms | Other loaded ms | Native probes recorded during run |
|---|---:|---:|---:|---:|---:|---:|---:|
| WiFiman | 674.30 | 790.56 | 23.0 | — | — | — | 369 |
| Speedtest.net | 694.83 | 727.27 | 13.0 | 158.0 | 25.0 | — | 371 |
| FAST.com | 800.00 | 750.00 | 16.0 | — | — | 26.0 | 271 |
| Devina bufferbloat | 528.72 | 540.49 | 20.9 | 31.6 | 31.6 | — | 749 |

- Devina's displayed increases were +10.70 ms for each direction. Its totals above are derived from the displayed 20.90 ms idle median. FAST's generic loaded value remains separate from the directional fields.
- The native cards and shared single-line log were inspected on the available desktop while the browser remained hidden. All four backend pages use the same Run/Cancel/result flow. Optional browser presentation remains inside the app.
- Adapter development encountered Devina's icon-only completed state and Speedtest's full-width result layout; both were corrected and retested successfully. Earlier diagnostic attempts were canceled and retained as canceled records.
- Redacted live-result fixtures cover all four providers. Integration tests confirm that provider identity and loaded-latency fields survive restart and JSON export, legacy rows remain readable, and late cancellation does not replace a completed record.
- All provider test data remained in the isolated preview directory; existing `dist/data` was preserved.
- The optimized build displayed all saved provider results in native cards. The Devina details dialog showed both loaded phases and focused Close on opening. A native Run action disabled competing tests; Cancel durably recorded cancellation and restored all Run actions.
- Current portable EXE: `dist/WANanalyzer.exe`, 15,240,704 bytes, SHA256 `055FE94A4FE59E058BD25D07B916811AECA1E2775A997BDB2C19B74830A425E7`. Final formatting check passed, and PE inspection confirmed embedded icon resources with only Windows/system imports.

## Scope of this evidence

These are local development checks, not full release qualification. Windows 10 clean-machine testing, a multi-display/DPI matrix, minimized-app and extended provider runs, soak testing, signing, and the broader PRD release gates remain open. Automatic capture depends on each provider's current rendered result layout.

Earlier desktop capture failed with `window crop is outside captured monitor`; those checks used accessibility state and read-only SQLite queries. Screenshots worked during the polishing pass, but coordinate input reported incorrect 16 × 16 window bounds. Keyboard input, accessible value editing, screenshots, and SQLite checks were used instead. The full small-window/high-DPI matrix and failure-path UI interactions remain release qualification work.
