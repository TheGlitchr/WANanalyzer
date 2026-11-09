//! Genuine iPerf3 TCP tests in a supervised, hidden process. No shell or browser.
use crate::{
    engine::Shared,
    model::{IperfDetails, IperfPhase, Outcome, SpeedRun, now_ms},
    platform::{self, Icmp},
    speed::BrowserResult,
    storage::Store,
};
use anyhow::{Context, Result, bail, ensure};
use crossbeam_channel::{Receiver, bounded};
use serde_json::Value;
use std::{
    io::Read,
    net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};

pub const PROVIDER: &str = "iPerf3 / Clouvider";
pub const VERSION: &str = "3.22";
const FIRST_PORT: u16 = 5200;
const PORTS: u16 = 10;
const OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
pub const SERVERS: [(&str, &str); 10] = [
    ("London", "lon.speedtest.clouvider.net"),
    ("Amsterdam", "ams.speedtest.clouvider.net"),
    ("Frankfurt", "fra.speedtest.clouvider.net"),
    ("New York", "nyc.speedtest.clouvider.net"),
    ("Los Angeles", "la.speedtest.clouvider.net"),
    ("Atlanta", "atl.speedtest.clouvider.net"),
    ("Dallas", "dal.speedtest.clouvider.net"),
    ("Ashburn", "ash.speedtest.clouvider.net"),
    ("Phoenix", "phx.speedtest.clouvider.net"),
    ("Chicago", "chi.speedtest.clouvider.net"),
];

#[derive(Clone, Debug)]
pub struct Options {
    pub auto_tune: bool,
    pub streams: u16,
    pub seconds: u32,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            auto_tune: true,
            streams: 32,
            seconds: 20,
        }
    }
}
impl Options {
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=128).contains(&self.streams),
            "Use 1–128 parallel streams"
        );
        ensure!(
            (10..=60).contains(&self.seconds),
            "Use 10–60 seconds per measured direction"
        );
        Ok(())
    }
}

pub struct Task {
    result: Receiver<BrowserResult>,
    cancel: Arc<AtomicBool>,
    phase: Arc<Mutex<String>>,
    saving: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Task {
    pub fn start(
        directory: PathBuf,
        store: Store,
        mut run: SpeedRun,
        network: Arc<Mutex<Shared>>,
        options: Options,
    ) -> Self {
        let (tx, result) = bounded(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let phase = Arc::new(Mutex::new(
            "Finding the lowest-latency Clouvider location".into(),
        ));
        let saving = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_phase = phase.clone();
        let worker_saving = saving.clone();
        let worker = thread::spawn(move || {
            let outcome = execute(
                &directory,
                &mut run,
                &options,
                &worker_cancel,
                &worker_phase,
            );
            let mut error = None;
            match outcome {
                Ok(()) => run.status = "completed".into(),
                Err(reason) => {
                    run.status = if worker_cancel.load(Ordering::Acquire) {
                        "canceled"
                    } else {
                        "capture_failed"
                    }
                    .into();
                    run.details.error = format!("{reason:#}");
                    error = Some(run.details.error.clone());
                }
            }
            run.finished_ms = Some(now_ms());
            run.end_context = network
                .lock()
                .unwrap()
                .targets
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}: {:?}", crate::model::TARGET_NAMES[i], t.network))
                .collect::<Vec<_>>()
                .join("; ");
            worker_saving.store(true, Ordering::Release);
            *worker_phase.lock().unwrap() = "Saving iPerf result".into();
            let saved = match store.speed(run.clone()) {
                Ok(()) => true,
                Err(reason) => {
                    error = Some(format!("Result NOT saved: {reason}"));
                    false
                }
            };
            let _ = tx.send(BrowserResult { run, saved, error });
        });
        Self {
            result,
            cancel,
            phase,
            saving,
            worker: Some(worker),
        }
    }
    pub fn phase(&self) -> String {
        self.phase.lock().unwrap().clone()
    }
    pub fn is_saving(&self) -> bool {
        self.saving.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn take_result(&self) -> Option<BrowserResult> {
        self.result.try_recv().ok()
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel();
        // The worker kills/reaps its child before persisting cancellation.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    location: &'static str,
    host: &'static str,
    ip: Ipv4Addr,
    latency: f64,
    method: &'static str,
}
fn check(cancel: &AtomicBool, deadline: Instant) -> Result<()> {
    ensure!(!cancel.load(Ordering::Acquire), "Test canceled");
    ensure!(
        Instant::now() < deadline,
        "iPerf test reached its four-minute time limit"
    );
    Ok(())
}
fn discover(cancel: &AtomicBool, deadline: Instant) -> Result<Vec<Candidate>> {
    let (tx, rx) = bounded(SERVERS.len());
    for (location, host) in SERVERS {
        let tx = tx.clone();
        thread::spawn(move || {
            let candidate = (|| -> Option<Candidate> {
                let ip =
                    (host, FIRST_PORT)
                        .to_socket_addrs()
                        .ok()?
                        .find_map(|addr| match addr.ip() {
                            std::net::IpAddr::V4(ip) => Some(ip),
                            _ => None,
                        })?;
                let mut samples = Vec::new();
                if let (Ok(icmp), Ok(route)) = (Icmp::new(), platform::route(ip)) {
                    for _ in 0..3 {
                        let (outcome, _, _, rtt) = icmp.probe(&route, 128, 600);
                        if outcome == Outcome::Reply
                            && let Some(rtt) = rtt
                        {
                            samples.push(f64::from(rtt));
                        }
                    }
                }
                let (latency, method) = if samples.is_empty() {
                    // Reachability/latency only; a successful TCP open does NOT mean the server is idle.
                    let mut measured = None;
                    for port in FIRST_PORT..FIRST_PORT + PORTS {
                        let start = Instant::now();
                        if let Ok(connection) = TcpStream::connect_timeout(
                            &SocketAddr::new(ip.into(), port),
                            Duration::from_millis(500),
                        ) {
                            measured = Some(start.elapsed().as_secs_f64() * 1000.);
                            drop(connection);
                            break;
                        }
                    }
                    let elapsed = measured?;
                    (elapsed, "TCP connect fallback")
                } else {
                    samples.sort_by(f64::total_cmp);
                    (samples[samples.len() / 2], "median ICMP RTT")
                };
                Some(Candidate {
                    location,
                    host,
                    ip,
                    latency,
                    method,
                })
            })();
            let _ = tx.try_send(candidate);
        });
    }
    drop(tx);
    let until = (Instant::now() + Duration::from_secs(9)).min(deadline);
    let mut candidates = Vec::new();
    loop {
        check(cancel, deadline)?;
        if Instant::now() >= until {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(candidate)) => candidates.push(candidate),
            Ok(None) => {}
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    candidates.sort_by(|a, b| {
        a.latency
            .total_cmp(&b.latency)
            .then_with(|| a.host.cmp(b.host))
    });
    ensure!(
        !candidates.is_empty(),
        "No Clouvider location responded. Check DNS, ICMP, and outbound TCP ports 5200–5209"
    );
    Ok(candidates)
}

fn ensure_runtime(directory: &Path) -> Result<PathBuf> {
    let runtime = directory.join("runtime").join("iperf3-3.22");
    std::fs::create_dir_all(&runtime).context("Create portable iPerf runtime folder")?;
    for (name, bytes) in [
        (
            "iperf3.exe",
            include_bytes!("../assets/iperf3/iperf3.exe").as_slice(),
        ),
        (
            "cygwin1.dll",
            include_bytes!("../assets/iperf3/cygwin1.dll").as_slice(),
        ),
        (
            "NOTICES.txt",
            include_bytes!("../assets/iperf3/NOTICES.txt").as_slice(),
        ),
    ] {
        let destination = runtime.join(name);
        if std::fs::read(&destination).is_ok_and(|existing| existing == bytes) {
            continue;
        }
        let temp = runtime.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temp, bytes).context("Extract embedded iPerf engine")?;
        if let Err(error) = std::fs::rename(&temp, &destination) {
            let _ = std::fs::remove_file(&temp);
            return Err(error).context("Replace portable iPerf runtime file");
        }
    }
    std::fs::canonicalize(runtime.join("iperf3.exe")).context("Locate extracted iPerf engine")
}

// Closing this Windows job kills the entire process tree even if the app crashes.
struct Job(HANDLE);
impl Job {
    fn attach(child: &Child) -> Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(!handle.is_null(), "Cannot create iPerf process job");
            let job = Self(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            ensure!(
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32
                ) != 0,
                "Cannot configure iPerf process job"
            );
            ensure!(
                AssignProcessToJobObject(handle, child.as_raw_handle().cast()) != 0,
                "Cannot supervise iPerf process"
            );
            Ok(job)
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Process {
    child: Child,
    job: Option<Job>,
}
impl Process {
    fn spawn(command: &mut Command) -> Result<Self> {
        let child = command
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Start embedded iPerf3")?;
        let mut process = Self { child, job: None };
        process.job = Some(Job::attach(&process.child)?);
        Ok(process)
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.job.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn arguments(
    ip: Ipv4Addr,
    port: u16,
    streams: u16,
    seconds: u32,
    omit: u32,
    download: bool,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "-c",
        &ip.to_string(),
        "-p",
        &port.to_string(),
        "-4",
        "-J",
        "-P",
        &streams.to_string(),
        "-t",
        &seconds.to_string(),
        "-O",
        &omit.to_string(),
        "-l",
        "128K",
        "--connect-timeout",
        "1500",
        "--rcv-timeout",
        "5000",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if download {
        args.push("-R".into());
    }
    args
}
fn invoke(exe: &Path, args: &[String], timeout: Duration, cancel: &AtomicBool) -> Result<Value> {
    let mut command = Command::new(exe);
    command.args(args).current_dir(exe.parent().unwrap());
    let mut process = Process::spawn(&mut command)?;
    let stdout = process
        .child
        .stdout
        .take()
        .context("iPerf stdout unavailable")?;
    let stderr = process
        .child
        .stderr
        .take()
        .context("iPerf stderr unavailable")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(16_385).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let outcome = loop {
        if cancel.load(Ordering::Acquire) {
            break Err(anyhow::anyhow!("Test canceled"));
        }
        if start.elapsed() > timeout {
            break Err(anyhow::anyhow!("iPerf process timed out"));
        }
        match process.child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => break Err(error.into()),
        }
    };
    drop(process); // kill/reap before joining pipe readers, on EVERY exit path
    let bytes = out
        .join()
        .map_err(|_| anyhow::anyhow!("iPerf output reader failed"))??;
    let errors = err
        .join()
        .map_err(|_| anyhow::anyhow!("iPerf error reader failed"))??;
    let status = outcome?;
    ensure!(
        bytes.len() as u64 <= OUTPUT_LIMIT,
        "iPerf output exceeded the 4 MiB limit"
    );
    let json: Value = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "iPerf returned invalid JSON: {}",
            String::from_utf8_lossy(&errors)
        )
    })?;
    if let Some(error) = json.get("error").and_then(Value::as_str) {
        bail!("iPerf: {error}");
    }
    ensure!(
        status.success(),
        "iPerf failed: {}",
        String::from_utf8_lossy(&errors)
    );
    Ok(json)
}
fn parse_phase(
    json: &Value,
    port: u16,
    streams: u16,
    seconds: u32,
    download: bool,
    calibration: bool,
) -> Result<IperfPhase> {
    let start = &json["start"]["test_start"];
    ensure!(
        start["protocol"].as_str() == Some("TCP")
            && start["num_streams"].as_u64() == Some(u64::from(streams)),
        "iPerf did not use the requested TCP stream count"
    );
    let reverse = start["reverse"]
        .as_u64()
        .map(|v| v != 0)
        .or_else(|| start["reverse"].as_bool());
    ensure!(reverse == Some(download), "iPerf result direction mismatch");
    let receiver = &json["end"]["sum_received"];
    let rate = receiver["bits_per_second"]
        .as_f64()
        .context("Missing iPerf receiver throughput")?;
    let elapsed = receiver["seconds"]
        .as_f64()
        .context("Missing iPerf measured duration")?;
    ensure!(
        rate.is_finite()
            && rate >= 0.
            && elapsed.is_finite()
            && elapsed >= f64::from(seconds) * 0.75,
        "Incomplete or invalid iPerf measurement"
    );
    let cpu = |key: &str| {
        json["end"]["cpu_utilization_percent"][key]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.)
    };
    Ok(IperfPhase {
        direction: if download { "download" } else { "upload" }.into(),
        calibration,
        streams,
        port,
        seconds: elapsed,
        receiver_mbps: rate / 1_000_000.,
        receiver_bytes: receiver["bytes"]
            .as_u64()
            .context("Missing iPerf receiver byte count")?,
        retransmits: json["end"]["sum_sent"]["retransmits"].as_u64(),
        local_cpu_percent: cpu("host_total"),
        remote_cpu_percent: cpu("remote_total"),
    })
}
fn retryable(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "busy",
        "unable to connect",
        "connection refused",
        "timed out",
        "control socket",
        "connection reset",
        "connection closed",
    ]
    .iter()
    .any(|reason| error.contains(reason))
}
struct Runner<'a> {
    exe: &'a Path,
    cancel: &'a AtomicBool,
    deadline: Instant,
    phase: &'a Mutex<String>,
    metadata: IperfDetails,
    raw: Vec<Value>,
    next_port: u16,
}
impl Runner<'_> {
    fn measure(
        &mut self,
        server: &Candidate,
        streams: u16,
        seconds: u32,
        calibration: bool,
        download: bool,
    ) -> Result<IperfPhase> {
        for _ in 0..PORTS {
            check(self.cancel, self.deadline)?;
            let port = FIRST_PORT + self.next_port % PORTS;
            self.next_port += 1;
            let direction = if download { "download" } else { "upload" };
            *self.phase.lock().unwrap() = format!(
                "{} {direction}: {streams} streams | {}:{port}",
                if calibration {
                    "Calibrating"
                } else {
                    "Measuring"
                },
                server.location
            );
            let args = arguments(
                server.ip,
                port,
                streams,
                seconds,
                if calibration { 1 } else { 2 },
                download,
            );
            let timeout = Duration::from_secs(u64::from(seconds) + 14)
                .min(self.deadline.saturating_duration_since(Instant::now()));
            match invoke(self.exe, &args, timeout, self.cancel) {
                Ok(json) => {
                    let measurement =
                        parse_phase(&json, port, streams, seconds, download, calibration)?;
                    // Reuse the successful instance for comparable stream trials;
                    // rotate only when that port is actually busy/unavailable.
                    self.next_port = port - FIRST_PORT;
                    self.raw.push(json);
                    self.metadata.phases.push(measurement.clone());
                    return Ok(measurement);
                }
                Err(error) => {
                    let message =
                        format!("{}:{port} {direction} P{streams}: {error:#}", server.host);
                    self.metadata.attempts.push(message);
                    check(self.cancel, self.deadline)?;
                    if !retryable(&error.to_string()) {
                        return Err(error);
                    }
                }
            }
        }
        bail!(
            "All Clouvider ports 5200–5209 were busy or unavailable in {}",
            server.location
        )
    }
}
fn choose_streams(phases: &[IperfPhase]) -> Option<u16> {
    let best = phases
        .iter()
        .filter(|p| p.calibration && p.direction == "download")
        .map(|p| p.receiver_mbps)
        .max_by(f64::total_cmp)?;
    // Prefer fewer streams if the result is within 3% of the best short trial.
    phases
        .iter()
        .filter(|p| p.calibration && p.direction == "download" && p.receiver_mbps >= best * 0.97)
        .map(|p| p.streams)
        .min()
}
fn execute(
    directory: &Path,
    run: &mut SpeedRun,
    options: &Options,
    cancel: &AtomicBool,
    phase: &Mutex<String>,
) -> Result<()> {
    options.validate()?;
    let deadline = Instant::now() + Duration::from_secs(240);
    let exe = ensure_runtime(directory)?;
    let candidates = discover(cancel, deadline)?;
    let ranking = candidates
        .iter()
        .map(|c| format!("{}: {:.1} ms ({})", c.location, c.latency, c.method))
        .collect::<Vec<_>>()
        .join("; ");
    let mut runner = Runner {
        exe: &exe,
        cancel,
        deadline,
        phase,
        metadata: IperfDetails {
            version: VERSION.into(),
            selection: ranking,
            duration_seconds: options.seconds,
            auto_tune: options.auto_tune,
            ..Default::default()
        },
        raw: Vec::new(),
        next_port: (now_ms() as u16) % PORTS,
    };
    let outcome = (|| -> Result<()> {
        let mut selected = None;
        for candidate in candidates {
            // Select by lowest measured latency with a successful real iPerf test.
            match runner.measure(
                &candidate,
                if options.auto_tune {
                    8
                } else {
                    options.streams
                },
                4,
                true,
                true,
            ) {
                Ok(_) => {
                    selected = Some(candidate);
                    break;
                }
                Err(error) => {
                    runner
                        .metadata
                        .attempts
                        .push(format!("{} skipped: {error:#}", candidate.location));
                    check(cancel, deadline)?;
                }
            }
        }
        let server =
            selected.context("No Clouvider server accepted an iPerf test on ports 5200–5209")?;
        runner.metadata.host = server.host.into();
        runner.metadata.location = server.location.into();
        run.server = format!("Clouvider {} ({})", server.location, server.host);
        run.ping_ms = Some(server.latency);
        run.details.latency_note = format!(
            "Server selection: {}. This is a pre-test latency sample, not an iPerf latency measurement. iPerf reports throughput; native ICMP monitoring continues separately.",
            server.method
        );
        if options.auto_tune {
            for streams in [16, 32, 64, 128] {
                if let Err(error) = runner.measure(&server, streams, 4, true, true) {
                    runner
                        .metadata
                        .attempts
                        .push(format!("Calibration P{streams} skipped: {error:#}"));
                    check(cancel, deadline)?;
                }
            }
        }
        let streams = if options.auto_tune {
            choose_streams(&runner.metadata.phases).context("No successful stream calibration")?
        } else {
            options.streams
        };
        runner.metadata.selected_streams = streams;
        let download = runner.measure(&server, streams, options.seconds, false, true)?;
        run.download_mbps = Some(download.receiver_mbps);
        let upload = runner.measure(&server, streams, options.seconds, false, false)?;
        run.upload_mbps = Some(upload.receiver_mbps);
        Ok(())
    })();
    run.details.raw_text = serde_json::to_string(&runner.raw)?;
    run.details.iperf = Some(runner.metadata);
    outcome?;
    run.validate_result()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    fn sample(rate: f64) -> Value {
        json!({"start":{"test_start":{"protocol":"TCP","num_streams":8,"reverse":1}},
            "end":{"sum_received":{"bits_per_second":rate,"seconds":20.1,"bytes":2000000000_u64},
            "sum_sent":{"retransmits":4},"cpu_utilization_percent":{"host_total":37.5,"remote_total":52.3}}})
    }
    #[test]
    fn receiver_summary_and_real_direction_are_required() {
        let json = sample(800_000_000.);
        let phase = parse_phase(&json, 5203, 8, 20, true, false).unwrap();
        assert_eq!(phase.receiver_mbps, 800.);
        assert_eq!(phase.retransmits, Some(4));
        assert_eq!(phase.local_cpu_percent, Some(37.5));
        assert!(parse_phase(&json, 5203, 8, 20, false, false).is_err());
        assert!(parse_phase(&json, 5203, 16, 20, true, false).is_err());
        let mut bad = json.clone();
        bad["end"].as_object_mut().unwrap().remove("sum_received");
        assert!(parse_phase(&bad, 5203, 8, 20, true, false).is_err());
        bad = json;
        bad["end"]["sum_received"]["seconds"] = json!(1.);
        assert!(parse_phase(&bad, 5203, 8, 20, true, false).is_err());
    }
    #[test]
    fn tuning_uses_smallest_stream_count_within_three_percent_of_peak() {
        let mut trials = Vec::new();
        for (streams, speed) in [(8, 700.), (16, 950.), (32, 960.), (64, 970.), (128, 900.)] {
            let mut trial =
                parse_phase(&sample(speed * 1_000_000.), 5200, 8, 20, true, true).unwrap();
            trial.streams = streams;
            trials.push(trial);
        }
        assert_eq!(choose_streams(&trials), Some(16));
        assert_eq!(choose_streams(&[]), None);
    }
    #[test]
    fn arguments_enable_reverse_parallel_tcp_without_unsupported_linux_options() {
        let args = arguments(Ipv4Addr::LOCALHOST, 5207, 128, 20, 2, true);
        assert!(args.windows(2).any(|pair| pair == ["-P", "128"]));
        assert!(args.contains(&"-R".into()));
        assert!(!args.contains(&"-u".into()));
        assert!(!args.contains(&"-Z".into()));
        assert!(!arguments(Ipv4Addr::LOCALHOST, 5207, 16, 20, 2, false).contains(&"-R".into()));
        assert!(retryable(
            "iperf: the server is busy running a test. try again later"
        ));
        assert!(!retryable("iperf: number of parallel streams too large"));
        assert!(
            Options {
                streams: 129,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    fn local_server(exe: &Path) -> (Process, u16) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut command = Command::new(exe);
        command.args([
            "-s",
            "-1",
            "-B",
            "127.0.0.1",
            "-p",
            &port.to_string(),
            "--idle-timeout",
            "8",
        ]);
        let process = Process::spawn(&mut command).unwrap();
        thread::sleep(Duration::from_millis(250));
        (process, port)
    }
    #[test]
    fn embedded_engine_transfers_parallel_tcp_both_directions_and_cancels() {
        let directory =
            std::env::temp_dir().join(format!("wanalyzer-iperf-test-{}", uuid::Uuid::new_v4()));
        let exe = ensure_runtime(&directory).unwrap();
        let cancel = AtomicBool::new(false);
        for download in [true, false] {
            let (_server, port) = local_server(&exe);
            let json = invoke(
                &exe,
                &arguments(Ipv4Addr::LOCALHOST, port, 4, 2, 0, download),
                Duration::from_secs(12),
                &cancel,
            )
            .unwrap();
            let measurement = parse_phase(&json, port, 4, 2, download, false).unwrap();
            assert!(measurement.receiver_mbps > 0.);
            assert!(measurement.receiver_bytes > 0);
        }
        let (server, port) = local_server(&exe);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            invoke(
                &exe,
                &arguments(Ipv4Addr::LOCALHOST, port, 4, 30, 0, true),
                Duration::from_secs(40),
                &worker_cancel,
            )
        });
        thread::sleep(Duration::from_millis(500));
        cancel.store(true, Ordering::Release);
        let at = Instant::now();
        assert!(
            worker
                .join()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("canceled")
        );
        assert!(at.elapsed() < Duration::from_secs(5));
        drop(server);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "Uses Internet bandwidth against Clouvider's public servers"]
    fn live_clouvider_auto_tune_with_monitoring_and_durable_history() {
        use crate::{
            engine::Monitor,
            model::{Settings, SpeedDetails},
            storage::Write,
        };
        let directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/iperf-live-check");
        let session = uuid::Uuid::new_v4().to_string();
        let store = Store::open(&directory, &session).unwrap();
        let monitor = Monitor::start(store.clone(), session.clone(), Settings::default());
        let run = SpeedRun {
            id: uuid::Uuid::new_v4().to_string(),
            provider: PROVIDER.into(),
            session,
            started_ms: now_ms(),
            finished_ms: None,
            status: "running".into(),
            download_mbps: None,
            upload_mbps: None,
            ping_ms: None,
            server: String::new(),
            result_url: String::new(),
            capture_method: "iperf3_json_v1".into(),
            start_context: String::new(),
            end_context: String::new(),
            details: SpeedDetails::default(),
        };
        store.speed(run.clone()).unwrap();
        let task = Task::start(
            directory.clone(),
            store.clone(),
            run,
            monitor.shared.clone(),
            Options::default(),
        );
        let result = task.result.recv_timeout(Duration::from_secs(260)).unwrap();
        std::fs::write(
            directory.join("result.json"),
            serde_json::to_vec_pretty(&result.run).unwrap(),
        )
        .unwrap();
        assert!(result.saved);
        assert_eq!(
            result.run.status, "completed",
            "{}",
            result.run.details.error
        );
        result.run.validate_result().unwrap();
        assert!(monitor.shared.lock().unwrap().submitted > 10);
        assert!(
            store
                .speeds()
                .unwrap()
                .iter()
                .any(|run| run.id == result.run.id && run.status == "completed")
        );
        println!(
            "Completed iPerf auto tuning; {} probes submitted concurrently",
            monitor.shared.lock().unwrap().submitted
        );
        drop(task);
        drop(monitor);
        let (tx, rx) = bounded(1);
        store.tx.send(Write::Close(tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
