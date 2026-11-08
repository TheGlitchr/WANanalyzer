# Changelog

## 0.1.0 — Preview

- Portable Rust desktop app for Windows 10/11 x64 with a bundled Signal Orbit icon.
- Automatic ping and repeated traceroute monitoring of Google, Cloudflare, and the discovered IPv4 gateway.
- Per-hop reply loss and latency charts, pause/resume controls, and configurable sampling.
- Native speed-test cards for WiFiman, Speedtest.net, FAST.com, and Devina bufferbloat, with one provider running at a time.
- Shared speed-test history and detailed provider measurements, including directional loaded latency where reported.
- Local SQLite sessions, settings, CSV/JSON exports, and consistent database backups.
- MIT-licensed source, Windows CI, and release packaging with dependency notices and SHA256 checksums.

This is an unsigned early preview. Speed tests require an existing Microsoft WebView2 Runtime; automatic capture depends on provider page layouts. IPv6, ARM64, long-range rollups, and the proposed storage-size budget are not implemented. Windows 10 clean-machine testing, broader display/DPI validation, and extended soak testing remain open. See [validation](VALIDATION.md) and the [README](README.md).
