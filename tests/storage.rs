use std::{net::Ipv4Addr, time::Duration};
use wanalyzer::{
    model::*,
    storage::{Store, Write, load_settings, save_settings},
};

fn sample(session: &str, sequence: u64) -> Observation {
    Observation {
        session: session.into(),
        target: 1,
        generation: 1,
        sequence,
        sweep: 0,
        sent_ms: now_ms(),
        finished_ms: now_ms(),
        ttl: None,
        network: Network {
            destination: Ipv4Addr::new(8, 8, 8, 8),
            source: Ipv4Addr::LOCALHOST,
            interface_index: 1,
            adapter: "test adapter".into(),
            next_hop: Ipv4Addr::LOCALHOST,
        },
        responder: Some(Ipv4Addr::LOCALHOST),
        outcome: Outcome::Reply,
        native_status: 0,
        rtt_ms: Some(5),
        timeout_ms: 1000,
    }
}

fn close(store: &Store) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    store.tx.send(Write::Close(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn durable_history_exports_and_terminal_speed_idempotence() {
    let directory = std::env::temp_dir().join(format!("wanalyzer-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let canonical = std::fs::canonicalize(&directory).unwrap();
    let store = Store::open(&canonical, "session-one").unwrap();
    let sample = Observation {
        session: "session-one".into(),
        target: 1,
        generation: 1,
        sequence: 1,
        sweep: 1,
        sent_ms: now_ms(),
        finished_ms: now_ms(),
        ttl: Some(1),
        network: Network {
            destination: Ipv4Addr::new(8, 8, 8, 8),
            source: Ipv4Addr::LOCALHOST,
            interface_index: 1,
            adapter: "test adapter".into(),
            next_hop: Ipv4Addr::LOCALHOST,
        },
        responder: Some(Ipv4Addr::LOCALHOST),
        outcome: Outcome::Hop,
        native_status: 11013,
        rtt_ms: Some(5),
        timeout_ms: 1000,
    };
    store.probe(sample);
    store.flush().unwrap();
    let read = store.observations("session-one", 0, 100).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].ttl, Some(1));
    assert_eq!(read[0].outcome, Outcome::Hop);
    let mut run = SpeedRun {
        id: "test-run".into(),
        provider: "WiFiman".into(),
        session: "session-one".into(),
        started_ms: now_ms(),
        finished_ms: None,
        status: "running".into(),
        download_mbps: None,
        upload_mbps: None,
        ping_ms: None,
        server: String::new(),
        result_url: String::new(),
        capture_method: "test".into(),
        start_context: String::new(),
        end_context: String::new(),
        details: SpeedDetails::default(),
    };
    store.speed(run.clone()).unwrap();
    run.status = "completed".into();
    run.finished_ms = Some(now_ms());
    run.download_mbps = Some(100.);
    run.upload_mbps = Some(10.);
    run.ping_ms = Some(5.);
    store.speed(run.clone()).unwrap();
    // Duplicate/stale completion must not rewrite the first terminal measurement.
    run.download_mbps = Some(999.);
    store.speed(run).unwrap();
    assert_eq!(store.speeds().unwrap()[0].download_mbps, Some(100.));
    // Export must not inherit the dashboard's 100-result presentation limit.
    let template = store.speeds().unwrap()[0].clone();
    {
        let mut db = store.read().unwrap();
        let tx = db.transaction().unwrap();
        for index in 0..101 {
            let mut item = template.clone();
            item.id = format!("older-run-{index}");
            tx.execute(
                "INSERT INTO speeds(id,session,at_ms,data) VALUES(?,?,?,?)",
                rusqlite::params![
                    item.id,
                    item.session,
                    item.started_ms,
                    serde_json::to_string(&item).unwrap()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    store.event(Event {
        at_ms: now_ms(),
        session: "session-one".into(),
        target: None,
        kind: "test_event".into(),
        message: "Export event provenance".into(),
    });
    store.flush().unwrap();
    assert_eq!(store.speeds().unwrap().len(), 100);
    store
        .export("session-one", &directory.join("history.json"), true)
        .unwrap();
    let export: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("history.json")).unwrap()).unwrap();
    assert_eq!(export["probes"].as_array().unwrap().len(), 1);
    assert_eq!(export["speed_tests"].as_array().unwrap().len(), 102);
    assert_eq!(export["events"].as_array().unwrap().len(), 1);
    store
        .export("session-one", &directory.join("history.csv"), false)
        .unwrap();
    assert_eq!(
        csv::Reader::from_path(directory.join("history.csv"))
            .unwrap()
            .records()
            .count(),
        1
    );
    store.backup(&directory.join("backup.db")).unwrap();
    let snapshot = rusqlite::Connection::open(directory.join("backup.db")).unwrap();
    assert_eq!(
        snapshot
            .query_row("SELECT COUNT(*) FROM probes", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let (tx, rx) = crossbeam_channel::bounded(1);
    store.tx.send(Write::Close(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // Retain this small test artifact in the OS temp directory for diagnostics.
}

#[test]
fn restart_marks_unfinished_tests_interrupted() {
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-recovery-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "first").unwrap();
    let run = SpeedRun {
        id: "unfinished".into(),
        provider: "WiFiman".into(),
        session: "first".into(),
        started_ms: now_ms(),
        finished_ms: None,
        status: "running".into(),
        download_mbps: None,
        upload_mbps: None,
        ping_ms: None,
        server: String::new(),
        result_url: String::new(),
        capture_method: "test".into(),
        start_context: String::new(),
        end_context: String::new(),
        details: SpeedDetails::default(),
    };
    store.speed(run).unwrap();
    let (tx, rx) = crossbeam_channel::bounded(1);
    store.tx.send(Write::Close(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let reopened = Store::open(&directory, "second").unwrap();
    assert_eq!(reopened.speeds().unwrap()[0].status, "interrupted");
    let (tx, rx) = crossbeam_channel::bounded(1);
    reopened.tx.send(Write::Close(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn outputs_cannot_overwrite_active_history_or_sqlite_working_files() {
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-protected-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "protected").unwrap();
    store.probe(sample("protected", 1));
    store.flush().unwrap();
    std::fs::create_dir(directory.join("alias-parent")).unwrap();
    for name in [
        "wanalyzer.db",
        "wanalyzer.db-wal",
        "wanalyzer.db-shm",
        "wanalyzer.db-journal",
    ] {
        // Existing files and nonexistent sidecars must be checked through a canonical parent.
        let path = directory.join("alias-parent").join("..").join(name);
        assert!(store.export("protected", &path, false).is_err(), "{name}");
        assert!(store.export("protected", &path, true).is_err(), "{name}");
        assert!(store.backup(&path).is_err(), "{name}");
        #[cfg(windows)]
        {
            let alternate_case = directory.join(name.to_ascii_uppercase());
            assert!(store.backup(&alternate_case).is_err(), "{name}");
        }
        assert_eq!(store.observations("protected", 0, 100).unwrap().len(), 1);
    }
    // Rejected exports must leave the writer usable, including its WAL/SHM files.
    store.probe(sample("protected", 2));
    store.flush().unwrap();
    assert_eq!(store.observations("protected", 0, 100).unwrap().len(), 2);
    close(&store);
}

#[test]
fn export_and_backup_include_probes_queued_immediately_before_request() {
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-fresh-output-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "fresh").unwrap();
    for sequence in 1..=200 {
        store.probe(sample("fresh", sequence));
    }
    // Deliberately omit flush: the export operation must create its own queue barrier.
    let json_path = directory.join("fresh.json");
    store.export("fresh", &json_path, true).unwrap();
    let output: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
    let probes = output["probes"].as_array().unwrap();
    assert_eq!(probes.len(), 200);
    assert!(probes.iter().any(|probe| probe["sequence"] == 200));

    store.probe(sample("fresh", 201));
    let csv_path = directory.join("fresh.csv");
    store.export("fresh", &csv_path, false).unwrap();
    assert_eq!(
        csv::Reader::from_path(&csv_path).unwrap().records().count(),
        201
    );
    store.probe(sample("fresh", 202));
    let backup_path = directory.join("fresh-backup.db");
    // Replacing an existing destination must work on Windows without truncating it first.
    std::fs::write(&backup_path, "previous snapshot").unwrap();
    store.backup(&backup_path).unwrap();
    let db = rusqlite::Connection::open(&backup_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM probes", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        202
    );
    drop(db);
    close(&store);
}

#[test]
fn failed_exports_preserve_existing_outputs_and_remove_temporary_files() {
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-output-failure-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "failure").unwrap();
    store.probe(sample("failure", 1));
    store.flush().unwrap();
    {
        let db = store.read().unwrap();
        // Exercise a failure after JSON has already written its probes section.
        db.execute(
            "INSERT INTO speeds(id,session,at_ms,data) VALUES('bad','failure',0,'invalid json')",
            [],
        )
        .unwrap();
        // Exercise a failure after CSV has written its header and first valid probe.
        db.execute(
            "INSERT INTO probes(session,target,generation,ttl,at_ms,data) VALUES('failure',1,1,NULL,?,'invalid json')",
            [now_ms() + 1],
        )
        .unwrap();
    }
    for (name, json) in [("existing.json", true), ("existing.csv", false)] {
        let path = directory.join(name);
        std::fs::write(&path, b"previous complete report").unwrap();
        assert!(store.export("failure", &path, json).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"previous complete report");
    }
    assert!(std::fs::read_dir(&directory).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
    close(&store);
}

#[test]
fn settings_replace_atomically_without_reusing_a_predictable_temporary_file() {
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-settings-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "settings").unwrap();
    let mut settings = Settings::default();
    save_settings(&directory, &settings).unwrap();
    // A stale predictable temporary name could even alias the live database.
    // Saving settings must never truncate that file or write JSON through the alias.
    let old_temporary = directory.join("settings.json.tmp");
    std::fs::hard_link(&store.path, &old_temporary).unwrap();
    settings.ping_ms = 2000;
    save_settings(&directory, &settings).unwrap();
    assert_eq!(load_settings(&directory).unwrap().ping_ms, 2000);
    assert!(
        std::fs::read(old_temporary)
            .unwrap()
            .starts_with(b"SQLite format 3")
    );
    store.probe(sample("settings", 1));
    store.flush().unwrap();
    assert_eq!(store.observations("settings", 0, 100).unwrap().len(), 1);
    close(&store);
}

#[cfg(windows)]
#[test]
fn locked_destinations_keep_the_previous_report_and_backup_intact() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory =
        std::env::temp_dir().join(format!("wanalyzer-locked-output-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&directory, "locked").unwrap();
    store.probe(sample("locked", 1));
    for (name, backup) in [("locked.json", false), ("locked.db", true)] {
        let path = directory.join(name);
        std::fs::write(&path, b"previous complete output").unwrap();
        let locked_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let result = if backup {
            store.backup(&path)
        } else {
            store.export("locked", &path, true)
        };
        assert!(result.is_err());
        drop(locked_file);
        assert_eq!(std::fs::read(&path).unwrap(), b"previous complete output");
    }
    assert!(std::fs::read_dir(&directory).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
    close(&store);
}
