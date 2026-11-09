# Native speed-test revision — November 8, 2026

The native UI now owns the four test cards and shared history. WiFiman, Speedtest.net, FAST.com, and Devina remain the actual measurement backends. A real WebView2 document runs each provider; no separate browser window, bundled Chromium installation, copied test engine, or OCR is required.

The iPerf revision adds a fifth card backed by a bundled, supervised process rather than WebView2. See [iPerf tuning and execution](IPERF.md). The browser-specific behavior below applies to the original four methods; all five share one active-test guard and durable history.

## Execution and capture

Clicking Run creates a durable attempt before opening the provider. Each adapter starts the provider through its ordinary page controls (or observes an automatic start), polls rendered DOM text/attributes, and recognizes completion. Completion requires a new active test followed by two matching result snapshots. Unknown or incomplete results are not recorded as successful measurements.

The browser is hidden by default with a fixed desktop viewport. Browser arguments disable background timer throttling and renderer backgrounding. These arguments are a mitigation, not a portable guarantee: Microsoft documents that hidden WebView2 pages can receive throttled work. Live hidden-browser checks are therefore part of validation. [Microsoft WebView2 visibility documentation](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2controller.isvisible)

Only one provider runs at once, avoiding self-created contention. The native monitor continues throughout. Browser initialization/navigation has a 45-second deadline; a run has a five-minute deadline. Cancel closes its browser and writes a terminal record. A failed commit keeps the result available for retry.

The optional Show provider page control displays the current document inside the same main window at 25% zoom. This supports consent and troubleshooting without changing the default native layout. Adapters do not solve challenges or choose privacy settings. Navigation is restricted to the provider's HTTPS hosts, permissions/downloads/new windows are blocked, and no native host bridge is exposed.

## Measurement semantics

| Provider | Base latency | Loaded latency |
|---|---|---|
| WiFiman | Provider-labeled Ping | Not reported separately by the observed result page. |
| Speedtest.net | Idle ping | Provider-reported total latency during download and upload. Added delay is calculated from each total and idle ping, with negative increases clamped to zero. |
| FAST.com | Unloaded latency | One provider-labeled Loaded value, retained without assigning an unsupported direction. |
| Devina | Displayed median idle latency | The page displays nonnegative added latency in each direction. The app retains those increases and derives each total as displayed idle plus displayed increase, rounded to the page's two-decimal precision. |

FAST's own explanation distinguishes unloaded and loaded latency, and says that its expanded view measures upload as well as download. [FAST.com explanation](https://fast.com/en/)

Devina describes its test as powered by Waveform. The observed page implementation displays median latency and the difference between each loaded median and the idle median. The app reads those displayed fields; it does not copy the throughput engine. [Devina test](https://devina.io/speed-test)

These are provider-path measurements, distinct from Windows ICMP RTT. A missing measurement stays missing. In particular, FAST's generic loaded measurement never fills download-loaded or upload-loaded fields. A successful Devina capture must contain both loaded phases. Provider choices of servers, transport, duration, and sampling can yield different throughput and latency values.

## History and presentation

SpeedRun retains its existing provider, throughput, ping, timestamps, network contexts, status, and result URL fields. A serde-defaulted details object adds loaded latency, increases, jitter when available, assessment text, measurement notes, raw result text, and failure reason. Existing SQLite rows remain readable without rewriting completed records. Session JSON exports include the new fields.

The shared log uses fixed single-line labels. Embedded whitespace is flattened and longer text truncates with a tooltip. A result details dialog provides the full measurements and captured text. Attempt statuses distinguish running, saved, canceled, interrupted, and failed.

## Local diagnostics

```powershell
./dist/WANanalyzer.exe --data-dir ./artifacts/provider-check --speed-check fast --speed-debug
```

Use `wifiman`, `speedtest`, `fast`, or `devina`. This opens the app with one test running in its hidden backend and closes after the attempt is saved. The explicit debug flag writes provider snapshots to `provider-debug.jsonl` in that data directory; these contain provider-visible text, including possible IP/server information. Ordinary runs do not create that debug file.

All four providers completed hidden-browser development checks locally. Parser regression fixtures are redacted copies of those observed result fields. This does not establish a supported provider API, distribution rights, long-term DOM stability, or completion of the Windows 10/11 and DPI release matrix.
