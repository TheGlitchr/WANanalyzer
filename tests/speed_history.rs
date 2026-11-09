use std::{collections::BTreeMap, time::Duration};
use wanalyzer::{
    model::{IperfDetails, IperfPhase, SpeedDetails, SpeedRun, now_ms},
    storage::{Store, Write},
};

fn run(provider: &str) -> SpeedRun {
    SpeedRun {
        id: uuid::Uuid::new_v4().to_string(),
        provider: provider.into(),
        session: "providers".into(),
        started_ms: now_ms(),
        finished_ms: None,
        status: "running".into(),
        download_mbps: Some(850.),
        upload_mbps: Some(700.),
        ping_ms: Some(12.),
        server: "Test server".into(),
        result_url: String::new(),
        capture_method: "fixture".into(),
        start_context: String::new(),
        end_context: String::new(),
        details: SpeedDetails::default(),
    }
}
fn close(store: &Store) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    store.tx.send(Write::Close(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn legacy_records_gain_empty_details_without_changing_existing_measurements() {
    let mut json = serde_json::to_value(run("WiFiman")).unwrap();
    json.as_object_mut().unwrap().remove("details");
    json.as_object_mut().unwrap().remove("provider");
    let recovered: SpeedRun = serde_json::from_value(json).unwrap();
    assert_eq!(recovered.provider, "WiFiman");
    assert_eq!(recovered.details, SpeedDetails::default());
    assert_eq!(recovered.download_mbps, Some(850.));
    assert_eq!(recovered.ping_ms, Some(12.));
}

#[test]
fn bufferbloat_requires_both_loaded_phases_and_rejects_invalid_result_metadata() {
    let mut result = run("Devina bufferbloat");
    assert!(result.validate_result().is_err());
    result.details.download_latency_ms = Some(20.);
    assert!(result.validate_result().is_err());
    result.details.upload_latency_ms = Some(35.);
    assert!(result.validate_result().is_ok());
    result.details.jitter_ms = Some(f64::INFINITY);
    assert!(result.validate_result().is_err());
    result.details.jitter_ms = None;
    result.result_url = "https://devina.io.evil.example/speed-test".into();
    assert!(result.validate_result().is_err());
    result.result_url = "https://devina.io/speed-test".into();
    assert!(result.validate_result().is_ok());
}

#[test]
fn all_provider_results_and_loaded_latency_survive_restart_and_json_export() {
    let temp = std::env::temp_dir();
    let directory = temp.join(format!("wanalyzer-provider-test-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "providers").unwrap();
    let mut expected = BTreeMap::new();
    for provider in [
        "WiFiman",
        "Speedtest.net",
        "FAST.com",
        "Devina bufferbloat",
        "iPerf3 / Clouvider",
    ] {
        let mut result = run(provider);
        store.speed(result.clone()).unwrap();
        result.status = "completed".into();
        result.finished_ms = Some(now_ms());
        result.details.raw_text = format!("{provider} completed result");
        if provider == "FAST.com" {
            result.details.loaded_latency_ms = Some(55.);
        }
        if matches!(provider, "Speedtest.net" | "Devina bufferbloat") {
            result.details.download_latency_ms = Some(40.);
            result.details.upload_latency_ms = Some(60.);
        }
        if provider == "Devina bufferbloat" {
            result.details.download_increase_ms = Some(28.);
            result.details.upload_increase_ms = Some(48.);
        }
        if provider == "iPerf3 / Clouvider" {
            result.details.iperf = Some(IperfDetails {
                version: "3.22".into(),
                selected_streams: 32,
                duration_seconds: 20,
                host: "test.example".into(),
                location: "Example".into(),
                phases: ["download", "upload"]
                    .into_iter()
                    .map(|direction| IperfPhase {
                        direction: direction.into(),
                        calibration: false,
                        streams: 32,
                        port: 5207,
                        seconds: 20.1,
                        receiver_mbps: if direction == "download" { 850. } else { 700. },
                        receiver_bytes: 2_000_000_000,
                        retransmits: Some(3),
                        local_cpu_percent: Some(35.),
                        remote_cpu_percent: Some(22.),
                    })
                    .collect(),
                ..Default::default()
            });
        }
        result.validate_result().unwrap();
        store.speed(result.clone()).unwrap();
        expected.insert(provider.to_string(), result.details.clone());
        // Late cancellation must never replace an already committed result.
        result.status = "canceled".into();
        result.download_mbps = None;
        store.speed(result).unwrap();
    }
    close(&store);
    let reopened = Store::open(&directory, "restart").unwrap();
    let results = reopened.speeds().unwrap();
    assert_eq!(results.len(), 5);
    for result in results {
        assert_eq!(result.status, "completed");
        assert_eq!(result.download_mbps, Some(850.));
        assert_eq!(result.details, expected[&result.provider]);
    }
    let destination = directory.join("providers.json");
    reopened.export("providers", &destination, true).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination).unwrap()).unwrap();
    let exported: Vec<SpeedRun> = serde_json::from_value(json["speed_tests"].clone()).unwrap();
    assert_eq!(exported.len(), 5);
    for result in exported {
        assert_eq!(result.details, expected[&result.provider]);
    }
    close(&reopened);
    assert!(directory.starts_with(&temp));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn high_precision_iperf_measurements_survive_database_restart_and_export_exactly() {
    let temp = std::env::temp_dir();
    let directory = temp.join(format!(
        "wanalyzer-iperf-precision-{}",
        uuid::Uuid::new_v4()
    ));
    let store = Store::open(&directory, "precision").unwrap();
    let mut expected = BTreeMap::new();
    let mut seed = 0x5a17_4f09_a831_286b_u64;
    for _ in 0..48 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        // Finite, physically plausible Mbps values with nontrivial mantissas.
        let rate = f64::from_bits(0x4080_0000_0000_0000 | (seed & 0x000f_ffff_ffff_ffff));
        let seconds = 20. + rate / 100_000.;
        let mut result = run("iPerf3 / Clouvider");
        result.status = "completed".into();
        result.finished_ms = Some(now_ms());
        result.download_mbps = Some(rate);
        result.upload_mbps = Some(rate / 1.17);
        result.ping_ms = Some(rate / 30.);
        result.session = "precision".into();
        result.details.iperf = Some(IperfDetails {
            version: "3.22".into(),
            selected_streams: 32,
            duration_seconds: 20,
            phases: ["download", "upload"]
                .into_iter()
                .map(|direction| IperfPhase {
                    direction: direction.into(),
                    calibration: false,
                    streams: 32,
                    port: 5204,
                    seconds,
                    receiver_mbps: if direction == "download" {
                        rate
                    } else {
                        rate / 1.17
                    },
                    receiver_bytes: 2_000_000_000,
                    retransmits: Some(7),
                    local_cpu_percent: Some(rate / 21.),
                    remote_cpu_percent: Some(rate / 27.),
                })
                .collect(),
            ..Default::default()
        });
        store.speed(result.clone()).unwrap();
        expected.insert(result.id.clone(), result);
    }
    close(&store);
    let reopened = Store::open(&directory, "restart").unwrap();
    for actual in reopened.speeds().unwrap() {
        let expected = &expected[&actual.id];
        assert_eq!(actual.download_mbps, expected.download_mbps);
        assert_eq!(actual.upload_mbps, expected.upload_mbps);
        assert_eq!(actual.ping_ms, expected.ping_ms);
        assert_eq!(actual.details, expected.details);
    }
    let destination = directory.join("precision.json");
    reopened.export("precision", &destination, true).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination).unwrap()).unwrap();
    let exported: Vec<SpeedRun> = serde_json::from_value(json["speed_tests"].clone()).unwrap();
    assert_eq!(exported.len(), expected.len());
    for actual in exported {
        assert_eq!(actual.details, expected[&actual.id].details);
        assert_eq!(actual.download_mbps, expected[&actual.id].download_mbps);
    }
    close(&reopened);
    assert!(directory.starts_with(&temp));
    std::fs::remove_dir_all(directory).unwrap();
}
