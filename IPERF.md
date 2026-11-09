# iPerf3 with Clouvider

The Speed tests tab includes a fifth native Run card. The app embeds a pinned, unmodified iPerf3 3.22 Windows x64 engine and Cygwin 3.6.10-1 runtime. Both extract to `data/runtime/iperf3-3.22` on first use. No installer, administrator privilege, separate download, or WebView2 runtime is needed for this method.

## Selection and port availability

The built-in catalog uses Clouvider's published London, Amsterdam, Frankfurt, New York, Los Angeles, Atlanta, Dallas, Ashburn, Phoenix, and Chicago hosts. Discovery resolves IPv4 addresses and measures three ICMP replies per location, using the median received RTT. When ICMP produces no reply, a bounded TCP-connect sample to an accessible published port is used instead. Locations are ranked by measured network latency, not GPS or IP geolocation. DNS/discovery work runs off the UI thread and is bounded by a nine-second collection window; unresolved/nonresponding locations are omitted.

The lowest-latency location that accepts an actual short iPerf download test is selected. Ports rotate across **5200–5209**, starting at a varying offset. A TCP connection alone does not prove a server is free: busy/connection errors from the actual iPerf attempt trigger the next port. If all published ports fail, initial selection advances to the next ranked location. Once selected, all calibration and measured phases use that same location/IP so results remain comparable. Each phase may use another available port on that location. A server that becomes unavailable later produces a logged failure with any earlier measurements retained. No ports outside the provider's published range are probed.

The displayed server-selection latency is a pre-test ICMP median or TCP-connect fallback, explicitly labeled in Details. It is not an iPerf latency measurement. iPerf does not populate the provider loaded-latency fields; continuous native ICMP monitoring remains available independently.

## Trying to saturate the connection

Default **Auto tune** runs sequential four-second download trials at 8, 16, 32, 64, and 128 parallel TCP streams, each after a one-second unmeasured warm-up. The highest-throughput trial is selected first; exact ties favor fewer streams. Rejected stream counts are recorded and skipped. The app runs a fresh 20-second download at that count, then compares a second full download using the strongest different trial count if enough time remains for upload. The faster full download determines the reported download speed and the stream count used for a separate 20-second upload. Each full phase excludes two seconds of warm-up. The visible results are receiver-side full measurements, not the highest calibration spike. Default total duration is approximately 100 seconds, plus any busy-port retries.

Turn off Auto tune to choose 1–128 streams. The manual count still gets a short availability test before the longer phases. The duration control requests 10–60 measured seconds per direction. Only one iPerf process/test runs at a time; no competing website tests start during it. Every run has a four-minute overall deadline.

The bundled engine's `MAX_STREAMS` is 128, so this reaches its maximum parallel stream count within one test. It does not start additional simultaneous tests on other public ports to exceed that limit. [ESnet 3.22 limits](https://github.com/esnet/iperf/blob/3.22/src/iperf.h).

The client uses TCP `-P`, download `-R`, JSON `-J`, an ordinary 128 KiB I/O buffer, IPv4, bounded connection/receive timeouts, and `-O` warm-up exclusion. TCP is uncapped by default. It preserves OS TCP window autotuning and does not change adapter settings, registry values, congestion control, system security, or process priority. Unsupported Windows zero-copy/Linux pacing switches are not enabled.

A failed second full-duration profile is logged without discarding a valid first download. All phases remain in history. A successful port is reused across phases until it becomes busy/unavailable, reducing comparisons between different server instances.

More streams can fill a latency-bound path or use more CPU cores, but may eventually reduce throughput. iPerf3 3.16+ uses a thread per stream; an older/limited server, CPU, NIC, Wi-Fi airtime, WAN path, or shared server can still be the bottleneck. Clouvider advertises **10 Gbps best effort**, not dedicated capacity. This feature cannot guarantee line rate, exceed a physical link limit, or establish that the nearest server is the fastest on every run. iPerf3 on Windows uses Cygwin and is not an officially supported ESnet platform; very high-speed Windows results need independent qualification.

## History, process lifetime, and portability

An attempt is committed before it starts. The usual shared history tracks running, completed, failed, canceled, and interrupted results. Structured metadata includes server ranking/method, location, selected streams, ports, trial/final receiver throughput and byte counts, durations, sender retransmits when supplied, available CPU statistics, and failed port attempts. Raw engine JSON remains in Details and JSON export. Legacy browser records deserialize with empty optional iPerf metadata. A completed result requires both full directional summaries; partial/canceled tests are never relabeled as successful.

The worker launches a fixed executable directly with argument arrays and no shell. Standard output/error are read concurrently with size limits. A Windows Job Object ends the process tree on cancellation, timeout, or app exit; the child is reaped before saving. Other tests stay disabled until saving completes. Save failures retain the result for Retry. The existing recording, export, and database behavior is shared with browser tests.

The binary archive's SHA256 and GitHub build-provenance attestation were verified during integration. The bundled DLL additionally matches the original Cygwin 3.6.10-1 binary package. [Notices](assets/iperf3/NOTICES.txt) record provenance and hashes. Unmodified corresponding source archives and the Cygwin build recipe accompany the repository and release ZIP under `assets/iperf3/sources`; preserve those sources and license texts when distributing this engine. The app's original Rust code remains MIT licensed.

## Primary sources

- [Clouvider's published speed-test hosts, ports, and capacity](https://as62240.net/speedtest)
- [Clouvider's throughput troubleshooting guidance](https://www.clouvider.com/knowledge_base/i-am-not-getting-10gbps-why/)
- [ESnet command reference](https://software.es.net/iperf/invoking.html) and [Windows/multithreading FAQ](https://software.es.net/iperf/faq.html)
- [Microsoft's Windows benchmarking caveats](https://techcommunity.microsoft.com/blog/networkingblog/three-reasons-why-you-should-not-use-iperf3-on-windows/4117876)
- [Windows build and provenance process](https://github.com/userdocs/iperf3-static/tree/3.22)
- [Cygwin licensing and linking exception](https://cygwin.com/licensing.html)
