use std::{collections::BTreeMap, time::Duration};
use wanalyzer::{
    model::{SpeedDetails, SpeedRun, now_ms},
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
    for provider in ["WiFiman", "Speedtest.net", "FAST.com", "Devina bufferbloat"] {
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
    assert_eq!(results.len(), 4);
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
    assert_eq!(exported.len(), 4);
    for result in exported {
        assert_eq!(result.details, expected[&result.provider]);
    }
    close(&reopened);
    assert!(directory.starts_with(&temp));
    std::fs::remove_dir_all(directory).unwrap();
}
