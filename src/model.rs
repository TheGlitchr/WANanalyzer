use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, net::Ipv4Addr};

pub const TARGET_NAMES: [&str; 4] = ["Google", "Google DNS", "Cloudflare DNS", "Default gateway"];
pub const TARGET_HOSTS: [&str; 4] = ["google.com", "8.8.8.8", "1.1.1.1", "Automatic"];

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    pub ping_ms: u64,
    pub trace_ms: u64,
    pub timeout_ms: u32,
    pub max_hops: u8,
    pub raw_hours: u32,
}
fn default_ui_scale() -> f32 {
    0.9
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: default_ui_scale(),
            ping_ms: 1_000,
            trace_ms: 5_000,
            timeout_ms: 1_000,
            max_hops: 30,
            raw_hours: 24,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (0.65..=1.35).contains(&self.ui_scale),
            "UI scale must be 65–135%"
        );
        anyhow::ensure!(
            (500..=60_000).contains(&self.ping_ms),
            "Ping interval must be 500–60000 ms"
        );
        anyhow::ensure!(
            (5_000..=300_000).contains(&self.trace_ms),
            "Trace interval must be 5000–300000 ms"
        );
        anyhow::ensure!(
            (100..=10_000).contains(&self.timeout_ms),
            "Timeout must be 100–10000 ms"
        );
        anyhow::ensure!((1..=64).contains(&self.max_hops), "Hop cap must be 1–64");
        anyhow::ensure!(
            (1..=168).contains(&self.raw_hours),
            "Raw retention must be 1–168 hours"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Network {
    pub destination: Ipv4Addr,
    pub source: Ipv4Addr,
    pub interface_index: u32,
    pub adapter: String,
    pub next_hop: Ipv4Addr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Reply,
    Hop,
    Timeout,
    RemoteError,
    LocalError,
}
impl Outcome {
    pub fn is_response(self) -> bool {
        matches!(self, Self::Reply | Self::Hop | Self::RemoteError)
    }
    pub fn counted(self) -> bool {
        self != Self::LocalError
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub session: String,
    pub target: usize,
    pub generation: u64,
    pub sequence: u64,
    pub sweep: u64,
    pub sent_ms: i64,
    pub finished_ms: i64,
    pub ttl: Option<u8>,
    pub network: Network,
    pub responder: Option<Ipv4Addr>,
    pub outcome: Outcome,
    pub native_status: u32,
    pub rtt_ms: Option<u32>,
    pub timeout_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub at_ms: i64,
    pub session: String,
    pub target: Option<usize>,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeedRun {
    pub id: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    pub session: String,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub status: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub server: String,
    pub result_url: String,
    pub capture_method: String,
    pub start_context: String,
    pub end_context: String,
    #[serde(default)]
    pub details: SpeedDetails,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeedDetails {
    pub download_latency_ms: Option<f64>,
    pub upload_latency_ms: Option<f64>,
    /// Some providers report one loaded value without identifying a direction.
    pub loaded_latency_ms: Option<f64>,
    pub download_increase_ms: Option<f64>,
    pub upload_increase_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub assessment: String,
    pub latency_note: String,
    pub raw_text: String,
    pub error: String,
    pub iperf: Option<IperfDetails>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IperfDetails {
    pub version: String,
    pub host: String,
    pub location: String,
    pub selection: String,
    pub selected_streams: u16,
    pub duration_seconds: u32,
    pub auto_tune: bool,
    pub phases: Vec<IperfPhase>,
    pub attempts: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IperfPhase {
    pub direction: String,
    pub calibration: bool,
    pub streams: u16,
    pub port: u16,
    pub seconds: f64,
    pub receiver_mbps: f64,
    pub receiver_bytes: u64,
    pub retransmits: Option<u64>,
    pub local_cpu_percent: Option<f64>,
    pub remote_cpu_percent: Option<f64>,
}
fn default_provider() -> String {
    "WiFiman".into()
}
impl SpeedRun {
    pub fn validate_result(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("download", self.download_mbps),
            ("upload", self.upload_mbps),
            ("ping", self.ping_ms),
        ] {
            if name == "ping" && self.provider == "iPerf3 / Clouvider" && value.is_none() {
                continue;
            }
            anyhow::ensure!(
                value.is_some_and(|v| v.is_finite() && v >= 0.0),
                "Invalid {name} measurement"
            );
        }
        if self.provider == "iPerf3 / Clouvider" {
            let details = self
                .details
                .iperf
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing iPerf metadata"))?;
            anyhow::ensure!(
                (1..=128).contains(&details.selected_streams),
                "Invalid iPerf stream count"
            );
            for direction in ["download", "upload"] {
                anyhow::ensure!(
                    details.phases.iter().any(|phase| !phase.calibration
                        && phase.direction == direction
                        && phase.receiver_mbps.is_finite()
                        && phase.receiver_mbps >= 0.
                        && phase.seconds.is_finite()
                        && phase.seconds > 0.),
                    "Missing completed iPerf {direction} phase"
                );
            }
        }
        for value in [
            self.details.download_latency_ms,
            self.details.upload_latency_ms,
            self.details.loaded_latency_ms,
            self.details.download_increase_ms,
            self.details.upload_increase_ms,
            self.details.jitter_ms,
        ]
        .into_iter()
        .flatten()
        {
            anyhow::ensure!(
                value.is_finite() && value >= 0.,
                "Invalid latency measurement"
            );
        }
        if !self.result_url.is_empty() {
            let url = url::Url::parse(&self.result_url)?;
            let allowed = match self.provider.as_str() {
                "WiFiman" => url.host_str() == Some("wifiman.com"),
                "Speedtest.net" => {
                    matches!(url.host_str(), Some("www.speedtest.net" | "speedtest.net"))
                }
                "FAST.com" => matches!(url.host_str(), Some("fast.com" | "www.fast.com")),
                "Devina bufferbloat" => url.host_str() == Some("devina.io"),
                _ => false,
            };
            anyhow::ensure!(
                allowed
                    && url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port_or_known_default() == Some(443),
                "Unexpected provider result URL"
            );
        }
        if self.provider == "Devina bufferbloat" {
            anyhow::ensure!(
                self.details.download_latency_ms.is_some()
                    && self.details.upload_latency_ms.is_some(),
                "Bufferbloat result requires both loaded latency measurements"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub sent: u64,
    pub responses: u64,
    pub lost: u64,
    pub local_errors: u64,
    pub remote_errors: u64,
    pub rtts: Vec<u32>,
    pub responders: BTreeSet<Ipv4Addr>,
    variation_sum: u64,
    variation_pairs: u64,
    previous: Option<(u64, u32)>,
}
impl Stats {
    pub fn add(&mut self, sample: &Observation) {
        if sample.outcome == Outcome::LocalError {
            self.local_errors += 1;
            self.previous = None;
            return;
        }
        self.sent += 1;
        // Hop PL measures absence of an ICMP response; remote errors are responses.
        // Endpoint PL measures missing successful echoes, and includes remote failures.
        let answered = if sample.ttl.is_some() {
            sample.outcome.is_response()
        } else {
            sample.outcome == Outcome::Reply
        };
        if answered {
            self.responses += 1;
        } else {
            self.lost += 1;
        }
        if sample.outcome == Outcome::RemoteError {
            self.remote_errors += 1;
        }
        if let Some(ip) = sample.responder {
            self.responders.insert(ip);
        }
        if matches!(sample.outcome, Outcome::Reply | Outcome::Hop)
            && let Some(rtt) = sample.rtt_ms
        {
            self.rtts.push(rtt);
            if let Some((seq, prev)) = self.previous
                && sample.sequence == seq + 1
            {
                self.variation_sum += rtt.abs_diff(prev) as u64;
                self.variation_pairs += 1;
            }
            self.previous = Some((sample.sequence, rtt));
            return;
        }
        self.previous = None;
    }
    pub fn loss(&self) -> Option<f64> {
        (self.sent > 0).then(|| 100.0 * self.lost as f64 / self.sent as f64)
    }
    pub fn mean(&self) -> Option<f64> {
        (!self.rtts.is_empty()).then(|| {
            self.rtts.iter().map(|v| *v as u64).sum::<u64>() as f64 / self.rtts.len() as f64
        })
    }
    pub fn p95(&self) -> Option<u32> {
        let mut v = self.rtts.clone();
        v.sort_unstable();
        (!v.is_empty()).then(|| v[(v.len() * 95).div_ceil(100) - 1])
    }
    pub fn variation(&self) -> Option<f64> {
        (self.variation_pairs > 0).then(|| self.variation_sum as f64 / self.variation_pairs as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn sample(
        ttl: Option<u8>,
        outcome: Outcome,
        sequence: u64,
        rtt: Option<u32>,
    ) -> Observation {
        Observation {
            session: "test".into(),
            target: 0,
            generation: 0,
            sequence,
            sweep: 0,
            sent_ms: 0,
            finished_ms: 1,
            ttl,
            network: Network {
                destination: Ipv4Addr::LOCALHOST,
                source: Ipv4Addr::LOCALHOST,
                interface_index: 1,
                adapter: "test".into(),
                next_hop: Ipv4Addr::LOCALHOST,
            },
            responder: outcome.is_response().then_some(Ipv4Addr::LOCALHOST),
            outcome,
            native_status: 0,
            rtt_ms: rtt,
            timeout_ms: 1000,
        }
    }
    #[test]
    fn hop_loss_counts_expired_replies_and_excludes_local_errors() {
        let mut s = Stats::default();
        for (i, outcome) in [
            Outcome::Hop,
            Outcome::Timeout,
            Outcome::LocalError,
            Outcome::RemoteError,
        ]
        .into_iter()
        .enumerate()
        {
            s.add(&sample(Some(2), outcome, i as u64, None));
        }
        assert_eq!((s.sent, s.responses, s.lost, s.local_errors), (3, 2, 1, 1));
        assert!((s.loss().unwrap() - 100.0 / 3.0).abs() < 1e-9);
    }
    #[test]
    fn endpoint_errors_are_not_zero_latency() {
        let mut s = Stats::default();
        s.add(&sample(None, Outcome::Reply, 1, Some(10)));
        s.add(&sample(None, Outcome::Timeout, 2, None));
        s.add(&sample(None, Outcome::Reply, 3, Some(80)));
        assert_eq!(s.mean(), Some(45.0));
        assert_eq!(s.p95(), Some(80));
        assert_eq!(s.variation(), None);
        assert_eq!(Stats::default().loss(), None);
    }
    #[test]
    fn remote_errors_differ_between_hops_and_endpoint() {
        let mut hop = Stats::default();
        let mut endpoint = Stats::default();
        hop.add(&sample(Some(3), Outcome::RemoteError, 1, None));
        endpoint.add(&sample(None, Outcome::RemoteError, 1, None));
        assert_eq!(hop.loss(), Some(0.0));
        assert_eq!(endpoint.loss(), Some(100.0));
    }
    #[test]
    fn settings_upgrade_keeps_existing_sampling_and_adds_scale() {
        let settings: Settings = serde_json::from_str(
            r#"{"ping_ms":2000,"trace_ms":10000,"timeout_ms":750,"max_hops":30,"raw_hours":48}"#,
        )
        .unwrap();
        assert_eq!(settings.ping_ms, 2000);
        assert_eq!(settings.ui_scale, 0.9);
        settings.validate().unwrap();
        let mut invalid = settings;
        invalid.ui_scale = f32::NAN;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn speed_results_reject_nonfinite_values() {
        let run = SpeedRun {
            id: "x".into(),
            provider: "WiFiman".into(),
            session: "x".into(),
            started_ms: 1,
            finished_ms: Some(2),
            status: "completed".into(),
            download_mbps: Some(f64::NAN),
            upload_mbps: Some(2.0),
            ping_ms: Some(5.0),
            server: String::new(),
            result_url: String::new(),
            capture_method: "manual".into(),
            start_context: String::new(),
            end_context: String::new(),
            details: SpeedDetails::default(),
        };
        assert!(run.validate_result().is_err());
    }
}
