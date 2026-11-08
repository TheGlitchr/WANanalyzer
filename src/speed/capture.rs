use super::{Capture, Provider, Snapshot};
use crate::model::SpeedDetails;
use anyhow::{Context, Result};

fn number(text: &str, units: &str) -> Result<f64> {
    let pattern = format!(r"(?i)([+]?[0-9]+(?:[.,][0-9]+)*)\s*({units})\b");
    let regex = regex::Regex::new(&pattern)?;
    let found = regex
        .captures(text)
        .with_context(|| format!("Missing measurement in {text:?}"))?;
    // Provider pages are requested in English: commas are thousands separators.
    let value: f64 = found[1].replace(',', "").parse()?;
    let unit = found[2].to_ascii_lowercase();
    let value = value
        * if unit.starts_with('g') {
            1000.
        } else if unit.starts_with('k') {
            0.001
        } else {
            1.
        };
    anyhow::ensure!(
        value.is_finite() && value >= 0.,
        "Invalid provider measurement"
    );
    Ok(value)
}
fn speed(text: &str) -> Result<f64> {
    number(text, r"Gbps|Mbps|Kbps|Gbit/s|Mbit/s|Kbit/s|Gb/s|Mb/s|Kb/s")
}
fn latency(text: &str) -> Result<f64> {
    number(text, "ms")
}
fn after_label(text: &str, label: &str, unit: &str) -> Result<f64> {
    let regex = regex::Regex::new(&format!(
        r"(?is)\b{label}\b[^0-9]{{0,70}}([0-9]+(?:[.,][0-9]+)*)\s*({unit})\b"
    ))?;
    let m = regex
        .captures(text)
        .with_context(|| format!("Missing completed {label} measurement"))?;
    number(&format!("{} {}", &m[1], &m[2]), unit)
}
pub(super) fn parse(provider: Provider, snapshot: &Snapshot) -> Result<Capture> {
    let f = |name: &str| snapshot.fields.get(name).map(String::as_str).unwrap_or("");
    let details = SpeedDetails {
        raw_text: snapshot.text.clone(),
        ..Default::default()
    };
    let mut result = match provider {
        Provider::WiFiman => {
            let mut result = super::parse_result(&snapshot.text)?;
            result.details = details;
            result.details.latency_note="WiFiman labels its value Ping. The result page does not identify separate download- and upload-loaded latency values.".into();
            result
        }
        Provider::Fast => Capture {
            download: speed(f("download"))?,
            upload: speed(f("upload"))?,
            ping: latency(f("ping"))?,
            server: f("server").into(),
            details: SpeedDetails {
                loaded_latency_ms: Some(latency(f("loaded"))?),
                latency_note: "FAST reports unloaded and loaded latency without identifying a separate loaded measurement for each direction.".into(),
                ..details
            },
        },
        Provider::Speedtest => {
            let text = if f("result_text").is_empty() {
                &snapshot.text
            } else {
                f("result_text")
            };
            let unit = r"Gbps|Mbps|Kbps";
            let download = if f("download").is_empty() {
                after_label(text, "Download", unit)?
            } else {
                speed(f("download"))?
            };
            let upload = if f("upload").is_empty() {
                after_label(text, "Upload", unit)?
            } else {
                speed(f("upload"))?
            };
            let ping = if f("ping").is_empty() {
                after_label(text, "(?:Idle|Ping)", "ms")?
            } else {
                latency(f("ping"))?
            };
            Capture {
                download,
                upload,
                ping,
                server: String::new(),
                details: SpeedDetails {
                    download_latency_ms: latency(f("download_latency")).ok(),
                    upload_latency_ms: latency(f("upload_latency")).ok(),
                    download_increase_ms:latency(f("download_latency")).ok().map(|value|(value-ping).max(0.)),
                    upload_increase_ms:latency(f("upload_latency")).ok().map(|value|(value-ping).max(0.)),
                    latency_note: "Speedtest reports total latency under download and upload load. Extra delay is calculated against idle ping; negative increases are shown as zero.".into(),
                    jitter_ms: latency(f("jitter")).ok(),
                    ..details
                },
            }
        }
        Provider::Devina => {
            let idle = latency(f("latency"))?;
            let download_increase = latency(f("download"))?;
            let upload_increase = latency(f("upload"))?;
            Capture {
                download: speed(f("download"))?,
                upload: speed(f("upload"))?,
                ping: idle,
                server: String::new(),
                details: SpeedDetails {
                    download_latency_ms: Some(((idle + download_increase)*100.).round()/100.),
                    upload_latency_ms: Some(((idle + upload_increase)*100.).round()/100.),
                    download_increase_ms: Some(download_increase),
                    upload_increase_ms: Some(upload_increase),
                    assessment: f("assessment").into(),
                    latency_note: "Devina displays median idle latency and nonnegative increases under load, rounded to two decimals. Loaded totals here are idle plus each displayed increase.".into(),
                    ..details
                },
            }
        }
    };
    // Avoid retaining a whole navigation/sidebar in the details view when a provider exposes a result card.
    if !f("result_text").is_empty() {
        result.details.raw_text = f("result_text").into();
    }
    Ok(result)
}

#[derive(Default)]
pub(super) struct ResultGate {
    active: bool,
    candidate: Option<Capture>,
}
impl ResultGate {
    pub fn observe(&mut self, provider: Provider, snapshot: &Snapshot) -> Result<Option<Capture>> {
        if snapshot.active {
            self.active = true;
            self.candidate = None;
        }
        if !self.active || !snapshot.complete {
            self.candidate = None;
            return Ok(None);
        }
        let mut candidate = parse(provider, snapshot)?;
        let text = std::mem::take(&mut candidate.details.raw_text);
        if self.candidate.as_ref() == Some(&candidate) {
            candidate.details.raw_text = text;
            Ok(Some(candidate))
        } else {
            self.candidate = Some(candidate);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_provider_result_fixtures_preserve_values_and_latency_directions() {
        for (provider, json) in [
            (
                Provider::WiFiman,
                include_str!("../../tests/fixtures/provider-wifiman.json"),
            ),
            (
                Provider::Speedtest,
                include_str!("../../tests/fixtures/provider-speedtest.json"),
            ),
            (
                Provider::Fast,
                include_str!("../../tests/fixtures/provider-fast.json"),
            ),
            (
                Provider::Devina,
                include_str!("../../tests/fixtures/provider-devina.json"),
            ),
        ] {
            let s: Snapshot = serde_json::from_str(json).unwrap();
            let expected: serde_json::Value = serde_json::from_str(json).unwrap();
            let expected = &expected["expected"];
            let c = parse(provider, &s).unwrap();
            for (value, key) in [
                (Some(c.download), "download"),
                (Some(c.upload), "upload"),
                (Some(c.ping), "ping"),
                (c.details.download_latency_ms, "download_loaded"),
                (c.details.upload_latency_ms, "upload_loaded"),
                (c.details.loaded_latency_ms, "loaded"),
            ] {
                match (value, expected[key].as_f64()) {
                    (Some(value), Some(expected)) => {
                        assert!((value - expected).abs() < 0.00001, "{provider:?} {key}")
                    }
                    (None, None) => {}
                    _ => panic!("Missing or invented {provider:?} {key}"),
                }
            }
        }
    }
    fn snapshot(fields: &[(&str, &str)]) -> Snapshot {
        Snapshot {
            origin: "https://fast.com".into(),
            text: "Completed test".into(),
            active: false,
            complete: true,
            phase: String::new(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
    #[test]
    fn fast_keeps_loaded_latency_unspecified_instead_of_inventing_a_direction() {
        let s = snapshot(&[
            ("download", "1.2 Gbps"),
            ("upload", "850 Mbps"),
            ("ping", "12 ms"),
            ("loaded", "74 ms"),
        ]);
        let c = parse(Provider::Fast, &s).unwrap();
        assert_eq!(c.download, 1200.);
        assert_eq!(c.details.loaded_latency_ms, Some(74.));
        assert_eq!(c.details.download_latency_ms, None);
    }
    #[test]
    fn devina_reports_added_latency_so_loaded_ping_includes_the_idle_baseline() {
        let s = snapshot(&[
            ("latency", "Latency\n23.50\nms"),
            ("download", "Download\n745.5\nMbps\n42.25\nms"),
            ("upload", "Upload\n900\nMbps\n120.0\nms"),
        ]);
        let c = parse(Provider::Devina, &s).unwrap();
        assert_eq!(c.details.download_latency_ms, Some(65.75));
        assert_eq!(c.details.upload_latency_ms, Some(143.5));
        assert_eq!(c.details.download_increase_ms, Some(42.25));
        let mut missing = s;
        missing.fields.remove("upload");
        assert!(parse(Provider::Devina, &missing).is_err());
    }
    #[test]
    fn fresh_result_gate_requires_activity_and_two_matching_completed_measurements() {
        let mut gate = ResultGate::default();
        let mut s = snapshot(&[
            ("download", "100 Mbps"),
            ("upload", "50 Mbps"),
            ("ping", "10 ms"),
            ("loaded", "40 ms"),
        ]);
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_none());
        s.active = true;
        s.complete = false;
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_none());
        s.active = false;
        s.complete = true;
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_none());
        s.complete = false;
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_none());
        s.complete = true;
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_none());
        s.text = "Completed test with a changed footer".into();
        assert!(gate.observe(Provider::Fast, &s).unwrap().is_some());
    }
}
