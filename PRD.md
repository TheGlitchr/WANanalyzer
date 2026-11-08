# WANanalyzer — Product requirements, v1

Status: implementation started after user authorization; this document remains the proposed full-v1 acceptance contract. See [README](README.md) for current coverage and qualification.
Research date: November 7, 2026.
Companion: [Technical research and delivery plan](TECHNICAL_RESEARCH.md).

Current speed-test revision (November 8): native cards and shared logging replace the four website quadrants. See [measurement semantics and implementation](NATIVE_SPEED_TESTS.md); original research-stage release gates below remain qualification work.

## 1. Product and intended outcome

Build a Windows network troubleshooting application, conceptually similar to PingPlotter, in Rust. Opening the application immediately starts continuous endpoint pings and repeated traceroute measurements against four default targets. An integrated, user-initiated WiFiman.com speed test records results in the same local history, allowing throughput measurements to be compared with latency, missed replies, and observed route changes.

The first release is a portable executable for Windows 10 and Windows 11. It needs no application installer, administrator rights, packet-capture driver, Rust installation, or separate database installation. The WiFiman feature may use an already installed Microsoft WebView2 Runtime, as confirmed by the user.

**Feasibility finding:** the native monitor has a credible implementation path. Automatic WiFiman result capture is an unresolved external integration requirement. A full v1 release must pass the WiFiman validation gate below; opening its website alone does not fulfill the request.

## 2. Confirmed requirements and planning assumptions

### Confirmed by the user

| Requirement | Interpretation |
|---|---|
| Windows application | Support Windows 10 and Windows 11. |
| Written in Rust | Rust application, native monitoring engine, UI, and persistence. The hosted provider page remains its own web application. |
| Portable EXE | One distributed EXE, no required installation; an existing WebView2 Runtime is acceptable for WiFiman. |
| Default targets | `google.com`, `8.8.8.8`, `1.1.1.1`, and the machine's discovered default gateway. |
| Start automatically | Every launch starts pinging and tracing all available default targets without a Start click. |
| Built-in speed-test option | Let the user run a genuine WiFiman.com test from the application. |
| Log speed results | Save structured results locally alongside monitoring history. |
| Current work | Implementation authorized after the research stage; include per-hop packet-loss tracking comparable to PingPlotter. |
| Four native provider cards | WiFiman, Speedtest.net, FAST.com, and Devina bufferbloat share a native result layout with individual Run actions. |
| Individual execution | One provider runs at a time in a background browser. No throughput tests start on app launch. |
| Shared speed log | Every provider attempt is logged. Fixed single-line rows open full details; Devina includes idle and both loaded-latency phases. |
| GitHub publication | Public WANanalyzer repository with the MIT license and a portable Windows preview download. |

### Proposed defaults, subject to product review

- Initial architecture: Windows x64, Windows 10 22H2 and Windows 11. ARM64 and 32-bit builds are later scope.
- IPv4 monitoring in v1. The three requested public targets have an IPv4 path; IPv6-only networks receive an explicit unsupported/no-IPv4-route state. Native IPv6 monitoring is a later extension, not an implied capability.
- Ping interval: 1 second. Full TTL-based trace sweep: every 5 seconds. Both begin during startup and continue independently of which target is selected.
- Manual speed tests only; launching the program does not start a throughput test.
- Default data directory: `data` beside the executable. Generated logs, configuration, and browser cache are normal portable application data files.
- No background Windows service or Windows-login autostart in v1. Closing the program stops measurement; minimizing leaves it running.
- Distribution: public GitHub source and preview binaries, as requested November 8. External provider integration qualification remains separate from the application's MIT license.

## 3. Users and core workflow

The primary user is someone troubleshooting intermittent home, small-office, or remote-work connectivity. They need to see whether symptoms affect the local gateway, several internet destinations, or one particular target, and how the connection behaves while a deliberate speed test loads it.

1. Run `WANanalyzer.exe` from a local folder or USB drive.
2. See the four target cards immediately, with recording status and the data location.
3. Endpoint samples arrive as replies/timeouts complete; a hop table and route history fill in as TTL probes complete.
4. Select any target or hop to inspect its latency timeline and response statistics.
5. Choose **Run WiFiman speed test**. The application opens its integrated provider window after checking the existing runtime.
6. Complete a test using WiFiman's genuine interface. Monitoring continues; the full test interval is marked on the plots.
7. Review the saved speed result, its timestamp and network context, then export selected history if needed.
8. Reopen the application later to start a new live session and inspect prior sessions.

## 4. Scope

### Required for full v1

- Four default target entries, all enabled and started on every launch. Pause/resume controls apply to the current session; v1 does not persist a paused state that would suppress the requested launch behavior.
- Concurrent endpoint monitoring and repeated TTL-based trace measurements for every enabled default target.
- Correct gateway discovery with visible adapter selection and explicit unavailable states.
- Target overview, selected-target hop table, synchronized timelines, route observations, and event markers.
- Integrated WiFiman.com test experience with automatic structured result capture, contingent on the release gate.
- Durable local monitoring, event, configuration, and speed-result history.
- CSV and JSON exports, reopening history, and bounded retention.
- Portable operation under a standard Windows user account.
- Explicit handling of offline startup, DNS failure, ICMP filtering, VPN/interface changes, sleep/resume, and logging failure.

### Later scope

- Native IPv6 probing; ARM64/32-bit binaries; Windows Server qualification.
- Arbitrary custom target management and broader target counts.
- TCP/UDP tracing, Paris traceroute, multipath reconstruction, packet capture, or driver installation.
- Scheduled/automatic speed tests, alerts, email, cloud storage, remote agents, accounts, or multi-device dashboards.
- Wireless signal/channel surveys and UniFi controller integration.
- PDF reports, automatic fault attribution, Windows service mode, tray-only operation, and automatic application updates.

The design may leave extension points for later features, but they must not delay the requested defaults or replace genuine WiFiman testing with another provider.

## 5. Functional requirements

### FR-01 — Immediate startup

- Show the native dashboard without waiting for DNS, WebView2, internet access, or a speed test.
- Start the two literal public IP targets as soon as the monitoring engine is ready. Resolve `google.com` and discover the gateway concurrently.
- Dispatch the first endpoint probe within 2 seconds of process launch on the reference test PC when its address and route are already available. This is a dispatch target, not a promise that the internet replies within 2 seconds.
- Dispatch a trace sweep for each available target within the same startup period. Show partial results as they arrive; do not wait for a complete route.
- Continue retries for unresolved/unavailable targets. A problem with one target must not stop the others.
- A second launch focuses the existing instance associated with the same data directory rather than silently doubling traffic and database writers.

### FR-02 — Default target identity

| Display name | Measurement address | Startup behavior |
|---|---|---|
| Google | Current selected A record for `google.com` | Resolve through Windows; display chosen IP and start immediately when available. |
| Google DNS | `8.8.8.8` | Start directly; do not require DNS. |
| Cloudflare DNS | `1.1.1.1` | Start directly; do not require DNS. |
| Default gateway | Current selected IPv4 default-route next hop | Discover at launch, display adapter, then start when a concrete address is available. |

- Keep stable logical target IDs while preserving the exact IP, interface, and network context used for each sample.
- A Google address change or gateway change creates a new address/network segment. Historical samples must retain their original identity.
- Never probe `0.0.0.0`, an unspecified next hop, or an arbitrary interface's gateway as if it were the selected gateway.
- Reverse-DNS hop names are optional display enrichment; absence must not delay collection.

### FR-03 — Continuous endpoint pings

- Send ICMP Echo Requests at a proposed 1-second interval, with 1,000 ms reply timeout and a small proposed 32-byte application payload.
- Record UTC time, sequence, logical target, actual destination/source context, outcome, native status, and RTT when available.
- Display last/minimum/mean/maximum RTT, p95, successful replies, finalized attempts, and ICMP probe non-success percentage for the selected window.
- Show the millisecond resolution of the Windows ICMP API honestly. A zero-millisecond result may display as `<1 ms`; do not imply measured microsecond precision.
- Never turn a timeout or error into a zero-RTT success. Pending, canceled, unsent, DNS-failed, and locally skipped work must remain distinguishable.
- Provide pause/resume per target and for the whole session; graph pauses as gaps.

### FR-04 — Repeated traceroute measurements

- Send probes to the original destination with TTL values from 1 to a proposed maximum of 30. Refresh these measurements every proposed 5 seconds.
- Treat a TTL-expired reply as a responding intermediate hop. Use its reply address and measured round-trip time.
- Stop extending the next sweep beyond a confirmed destination hop. Rediscover additional TTLs if the path changes or that termination is no longer observed.
- Missing replies at one TTL must not prevent measurement of later TTLs.
- Re-measure hops through TTL-limited probes to the target. Directly pinging discovered router addresses is not an equivalent substitute.
- Maintain observations by target/address segment, sweep, TTL, and responder. Display multiple observed responders at a TTL without pretending they form one proven physical route.
- Show last/minimum/mean/maximum hop RTT, reply count, and probe non-response percentage. Keep these separate from endpoint Echo statistics.
- A gateway trace normally terminates at hop 1; handle a nonresponding gateway normally rather than assuming success.

### FR-05 — Gateway and interface changes

- Select the machine's best eligible IPv4 default route using Windows route/interface information and effective metrics. Show the chosen adapter and gateway.
- Record the actual Windows-selected egress/source for each public destination separately. VPN or policy routes can make it different from the displayed default route.
- If the selected route is on-link or virtual with no concrete gateway address, show that condition; do not silently replace it with a lower-priority physical gateway.
- Observe interface, route, and address changes. Re-resolve/reselect and start a new network segment when needed.
- Mark changes on timelines. Preserve earlier data and do not label a gateway switch as one gateway's packet loss.
- Do not force a network adapter or change the machine's routing table in v1.

### FR-06 — Plotting and interpretation

- Show four compact overview timelines and a selected-target hop table above compact latency/loss charts. Open maximized with adjustable interface scale; all hop columns and multiple rows must be visible without scrolling the whole page.
- Offer 10-minute, 1-hour, 24-hour, and retained-history ranges with synchronized cursor/zoom and exact timestamps in tooltips.
- Plot successful RTT as latency; mark missed probes separately. Use gaps for periods without measurements.
- Mark speed-test intervals, pause/resume, network changes, application interruption, and sleep/resume.
- Highlight endpoint symptoms first. Intermediate-router non-response by itself is not proof of downstream forwarding loss; ICMP paths also do not establish the reverse path or application performance. [Pingman explanation](https://www.pingman.com/kb/24)
- Label the proposed jitter statistic **RTT variation** and expose its definition. Do not equate it with provider jitter or standards-based one-way delay variation.
- Downsample for display while retaining extrema, missing-reply counts, and event boundaries. Do not draw a continuous line through outages.

### FR-07 — Provider execution and logging

- Provide a local speed-test panel immediately; initialize the provider/browser only after user action.
- Present four native Run cards with common fields. Genuine provider pages execute in a hidden WebView2 backend, and provider-specific DOM adapters capture their completed measurements.
- Provide an optional same-window provider view for notices, consent, interaction, or troubleshooting. Never require a separate browser window or an HTML iframe. Keep native monitoring independent.
- Check the existing WebView2 Runtime. If unavailable, log the failed attempt and leave native monitoring active.
- Make provider notices and controls accessible through the optional view. Ordinary subsequent runs do not require a repeated app permission dialog.
- Allow only one app-tracked speed-test run at a time. Monitoring continues throughout.
- Track one provider test per embedded-window lifetime. End that test surface after its first terminal outcome and return to the local result/status view; another test starts through the app's Run action with a fresh durable run record.
- Persist run initiation before launching provider navigation; persist the terminal result before showing **Saved**.
- Required successful result fields: local run ID, provider, start/end UTC, download Mbps, upload Mbps, provider ping ms, capture method/version, associated monitoring session, and start/end network context.
- Server identity/location, provider result URL/ID, provider jitter, loaded latency, and byte counts are optional and nullable when genuinely available. Do not manufacture missing values.
- Devina is the exception to optional loaded latency: a successful bufferbloat capture requires idle, download-loaded, and upload-loaded latency. Retain its reported increases as well as the derived totals. FAST's generic loaded value must not populate either direction without evidence.
- Log every provider attempt on the left with consistent single-line fields and truncation/tooltips; allow full result details without wrapping the log itself.
- Distinguish completed-and-saved, canceled, failed, interrupted, and completed-but-capture-failed states. Failed/canceled tests must not appear as zero-Mbps successful tests.
- If the interface changes during the run, mark the result as mixed network context.
- Reject stale, duplicate, malformed, or unrecognized result data. A manually entered result must be labeled manual and cannot count as automatic capture acceptance.
- Ignore a result already visible on initial navigation. Require a new in-session provider start-to-completion transition or an equally reliable new provider run identity; a local nonce by itself does not prove result freshness.

### FR-08 — Local history and retention

- Save all dispatched endpoint and TTL observations, route/address events, sessions, speed-test records, and configuration in an embedded local database.
- Batch writes on a dedicated worker; proposed flush interval is at most 1 second. Speed-test start and terminal records require acknowledged commits.
- Default retention proposal: raw probe observations for 24 hours; 1-minute summaries for 30 days; speed-test records for 365 days. Let the user adjust these values and clearly state when detail has expired.
- Proposed database budget: 2 GiB, with warning at 80%. Trim eligible old raw data first. Account for WAL growth and free disk space; the budget is an operating target, not a claim of a filesystem hard quota.
- Never silently discard measurements or claim recording is active after writes fail. Display an obvious unsaved/memory-only state and preserve explicit collection gaps where possible.
- Support session/history review after restart. Reconcile an unfinished speed-test run as interrupted.
- Reclaim/reuse database space through a deliberate maintenance policy; deleting rows alone is not assumed to shrink the file.

### FR-09 — Portable data and exports

- Distribute one EXE. Store writable data in a clearly visible `data` folder beside it by default, including a separate browser-data subfolder.
- If that path is read-only, show a folder selector and an explicit local-application-data option. Measurement may start in bounded memory meanwhile, with a persistent **Not saved** indicator.
- Reject active database locations on network shares for the proposed SQLite WAL mode. The executable may reside elsewhere if the selected data directory is local and writable.
- Export selected intervals/targets as CSV and JSON with schema version, UTC timestamps, units, outcomes, exact addresses, and relevant network context. Keep speed-test and probe record types identifiable.
- Provide a consistent database snapshot export rather than instructing users to copy a live database file without its WAL.
- Explain what history includes before export; include no browser cookies, credentials, or unrelated browsing data.

## 6. Metric definitions

| Metric | Required definition |
|---|---|
| Endpoint RTT | Native ICMP RTT for a successful Echo Reply; milliseconds. |
| Mean/min/max/p95 | Successful RTT samples only; expose sample count and selected time window. Use a documented nearest-rank p95 for raw samples. |
| ICMP probe non-success % | `(timeouts + explicit remote network-error outcomes) / (successful echoes + timeouts + explicit remote network-error outcomes) × 100`. Exclude local failures, unsent work, canceled work, pending probes, and application gaps. This is not proven application-packet loss. |
| Hop non-response % | `timeouts / (TTL-expired replies + endpoint echoes + explicit remote error replies + timeouts) × 100` at the selected TTL/segment. Exclude local/unknown-provenance errors, pending, canceled, and skipped work. Remote error replies count as responses but retain a separate error status/count. |
| RTT variation | Mean absolute difference of adjacent successful endpoint RTTs within the same uninterrupted segment. Do not bridge missing samples, pauses, or address/network changes; null with fewer than one valid pair. |
| Throughput | Provider-reported download/upload, normalized to decimal Mbps; preserve capture provenance and provider precision. |
| Provider latency | WiFiman-reported latency, separately named; never overwrite ICMP RTT with it. |

One-minute summaries must retain counts, RTT sum, extrema, and variation pair counts/sums. Do not average averages. Raw p95 cannot be reconstructed exactly from those summaries; label aggregate p95 unavailable unless a separately validated histogram/sketch is added.

An empty denominator or absent valid RTT sample displays as unavailable, not zero. Unknown-provenance API errors are excluded from both endpoint and hop network-rate denominators.

## 7. Performance and reliability targets

These are proposed acceptance targets, not measured results. Use a documented reference PC with SSD, four cores, and 8 GB RAM.

| Area | Proposed acceptance target |
|---|---|
| Startup | Native dashboard and first available-target dispatch within 2 seconds. DNS and internet responses are outside this dispatch target. |
| Probe budget | Default ceiling 40 outbound probes/second globally, staggered; at most 32 in flight. Endpoint work takes priority. |
| Scheduling | No overlap between one target's trace sweeps; missed work is recorded as a scheduling gap, never replayed in a catch-up burst. |
| Responsiveness | Target switching, pause, and plot interactions respond within 100 ms under the default workload. |
| Native-core resources | Target under 5% average total-machine CPU and under 200 MiB steady-state working set on the reference PC; browser subprocess use reported separately. |
| Collection durability | Acknowledged commits survive process termination; normally no more than the uncommitted 1-second probe batch is lost on process crash. Power-loss guarantees depend on the chosen SQLite sync policy and storage behavior. |
| Endurance | 24-hour default-workload soak without unbounded memory/queue/file growth or collection stalls. |
| Portability | Clean standard-user Windows 10/11 x64 VMs run the native core without developer tools, a VC redistributable install, or non-system DLL sidecars. |

## 8. Release acceptance scenarios

| ID | Scenario and pass condition |
|---|---|
| AC-01 | Fresh launch: all four default rows exist; available targets begin endpoint and TTL measurement without a click. |
| AC-02 | DNS unavailable: IP targets and discoverable gateway continue; Google shows a DNS failure, not invented packet loss. |
| AC-03 | Lab trace with known hops: each replying TTL has the correct address/status; destination termination and maximum-hop behavior are correct. |
| AC-04 | One router suppresses TTL replies but destination answers: later hops still appear; UI does not declare the router a forwarding fault. |
| AC-05 | Destination stops replying: timed-out probes appear as missing replies, not zero latency; mean and loss denominators match the definitions. |
| AC-06 | VPN, Ethernet/Wi-Fi switch, DHCP gateway change, or equal-metric defaults: deterministic displayed selection, retained exact egress context, and separate historical segments. |
| AC-07 | Sleep/resume and manual clock adjustment: no artificial mass packet-loss interval; UTC events remain interpretable and scheduling resumes without a burst. |
| AC-08 | Working WiFiman integration: at least 10 completed tests across wired and Wi-Fi connections on Windows 10/11 match the provider's displayed values within its rounding, with exactly one durable completed record per run. |
| AC-09 | Cancel, offline launch, network switch, duplicate completion, unknown provider page, cached result on initial load, rerun attempts, and provider outage: correct terminal state, fresh per-test identity, and no fabricated or stale result. |
| AC-10 | WebView2 missing: native monitoring starts normally; speed test is explicitly unavailable. The accepted runtime prerequisite is documented. |
| AC-11 | Speed test under load: endpoint/TTL measurements continue and the test interval is visible in current and reopened history. |
| AC-12 | Restart, process kill, read-only folder, disk full, and database write failure: earlier committed history survives and any unsaved period is visible. |
| AC-13 | USB/local portable launch, spaces and non-ASCII paths, no installed VC runtime, remote desktop, and high-DPI displays: application works within the stated support matrix. |
| AC-14 | CSV/JSON and database snapshot reopen with correct units, outcomes, segment identities, and speed-test associations. |
| AC-15 | 24-hour soak, retention rollover, and budget pressure meet responsiveness and bounded-resource requirements. |

## 9. WiFiman release gate

Before describing the full requested product as ready to implement without qualification, validate:

1. Ubiquiti's permitted integration/distribution conditions, including embedded browsing and automated local capture, for the intended audience.
2. A supported API/callback, or a provider-permitted, versioned capture adapter that can identify completed results reliably.
3. Genuine operation within a top-level WebView2 window on both supported operating systems.
4. Durable capture, accurate units, cancellation/interruption behavior, and all AC-08 through AC-11 cases.

Research found no publicly documented WiFiman client API or result callback; that is a search finding, not proof none exists. The site's currently observed framing restriction precludes an iframe design. [WiFiman](https://wifiman.com/), [Ubiquiti terms](https://www.ui.com/legal/termsofservice/), [WebView2 capabilities](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/javascript)

If this gate fails, the remaining monitor is still a viable separately scoped product. External-browser launch, manual result entry, and a different speed-test provider are contingency options requiring an explicit scope change; none fulfills automatic WiFiman logging by itself.

## 10. Decisions still open

- Actual distribution audience and the corresponding WiFiman integration arrangement.
- Acceptance of the proposed x64/IPv4 v1 baseline, sampling intervals, retention, and data directory behavior.
- Final UI/plot dependency choice after native-rendering and browser-hosting spikes.
- Code-signing identity. The application's public name is WANanalyzer and its license is MIT, confirmed November 8.

These decisions do not prevent documenting or prototyping the native monitor after coding is authorized. Provider clarification and the WiFiman spike should come first because they can change the complete product's feasibility.
