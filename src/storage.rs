use crate::model::*;
use anyhow::{Context, Result};
use crossbeam_channel::{Sender, bounded};
use rusqlite::{Connection, params};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Default, Clone)]
pub struct Health {
    pub saved: u64,
    pub dropped: u64,
    pub error: Option<String>,
}
pub enum Write {
    Probe(Observation),
    Event(Event),
    Speed(Box<SpeedRun>, Sender<Result<(), String>>),
    Flush(Sender<Result<(), String>>),
    Close(Sender<()>),
}
#[derive(Clone)]
pub struct Store {
    pub path: PathBuf,
    pub tx: Sender<Write>,
    pub health: Arc<Mutex<Health>>,
}
#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub started: i64,
    pub ended: Option<i64>,
}

impl Store {
    pub fn open(directory: &Path, session: &str) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let directory = std::fs::canonicalize(directory).context("Resolve data folder")?;
        let is_unc = matches!(directory.components().next(),Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(),std::path::Prefix::UNC(..)|std::path::Prefix::VerbatimUNC(..)));
        anyhow::ensure!(!is_unc, "Use a local data folder, not a network share");
        let path = directory.join("wanalyzer.db");
        let mut db = Connection::open(&path).context("Open history database")?;
        db.busy_timeout(Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, started INTEGER NOT NULL, ended INTEGER);
            CREATE TABLE IF NOT EXISTS probes(id INTEGER PRIMARY KEY, session TEXT NOT NULL, target INTEGER NOT NULL, generation INTEGER NOT NULL, ttl INTEGER, at_ms INTEGER NOT NULL, data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS probes_time ON probes(at_ms);
            CREATE INDEX IF NOT EXISTS probes_session_target ON probes(session,target,at_ms);
            CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, session TEXT, at_ms INTEGER, data TEXT);
            CREATE TABLE IF NOT EXISTS speeds(id TEXT PRIMARY KEY, session TEXT, at_ms INTEGER, data TEXT);
            PRAGMA user_version=1;")?;
        // Reconcile interrupted sessions and speed tests without rewriting completed data.
        let interrupted: Vec<(String, String)> = {
            let mut q = db.prepare(
                "SELECT id,data FROM speeds WHERE json_extract(data,'$.status')='running'",
            )?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (id, data) in interrupted {
            let mut run: SpeedRun = serde_json::from_str(&data)?;
            run.status = "interrupted".into();
            run.finished_ms = Some(now_ms());
            db.execute(
                "UPDATE speeds SET data=? WHERE id=?",
                params![serde_json::to_string(&run)?, id],
            )?;
        }
        db.execute("UPDATE sessions SET ended=COALESCE((SELECT MAX(at_ms) FROM probes WHERE probes.session=sessions.id),started) WHERE ended IS NULL", [])?;
        db.execute(
            "INSERT INTO sessions(id,started) VALUES(?,?)",
            params![session, now_ms()],
        )?;
        let (tx, rx) = bounded(8192);
        let health = Arc::new(Mutex::new(Health::default()));
        let worker_health = health.clone();
        let sid = session.to_owned();
        let data_directory = directory;
        thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || {
                let mut batch = Vec::new();
                let mut last_flush = Instant::now();
                let mut last_prune = Instant::now();
                loop {
                    let message = rx.recv_timeout(Duration::from_millis(100));
                    let mut ack = None;
                    let mut closing = false;
                    let mut close_ack = None;
                    match message {
                        Ok(Write::Speed(run, reply)) => {
                            batch.push(Write::Speed(run, reply.clone()));
                            ack = Some(reply);
                        }
                        Ok(Write::Flush(reply)) => ack = Some(reply),
                        Ok(Write::Close(reply)) => {
                            closing = true;
                            close_ack = Some(reply);
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => closing = true,
                        Ok(other) => batch.push(other),
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    }
                    if last_flush.elapsed() >= Duration::from_secs(1)
                        || batch.len() >= 512
                        || ack.is_some()
                        || closing
                    {
                        let result = flush_batch(&mut db, &batch).map_err(|e| e.to_string());
                        let mut h = worker_health.lock().unwrap();
                        match &result {
                            Ok(()) => {
                                h.saved += batch.len() as u64;
                                if !batch.is_empty() {
                                    h.error = None;
                                }
                            }
                            Err(e) => {
                                h.dropped += batch.len() as u64;
                                h.error = Some(e.clone());
                            }
                        }
                        drop(h);
                        if let Some(reply) = ack {
                            let _ = reply.send(result);
                        }
                        batch.clear();
                        last_flush = Instant::now();
                    }
                    if last_prune.elapsed() > Duration::from_secs(60) {
                        // Raw retention and budget pressure; preserve speed history separately.
                        let retention =
                            load_settings(&data_directory).unwrap_or_default().raw_hours;
                        let cutoff = now_ms() - retention as i64 * 3_600_000;
                        if let Err(e) = db.execute("DELETE FROM probes WHERE at_ms < ?", [cutoff]) {
                            worker_health.lock().unwrap().error = Some(e.to_string());
                        }
                        let _ = db.execute(
                            "DELETE FROM speeds WHERE at_ms < ?",
                            [now_ms() - 365 * 86_400_000i64],
                        );
                        let _ = db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
                        last_prune = Instant::now();
                    }
                    if closing {
                        let _ = db.execute(
                            "UPDATE sessions SET ended=? WHERE id=?",
                            params![now_ms(), sid],
                        );
                        let _ = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
                        if let Some(reply) = close_ack {
                            let _ = reply.send(());
                        }
                        break;
                    }
                }
            })?;
        Ok(Self { path, tx, health })
    }
    pub fn probe(&self, probe: Observation) {
        if self.tx.try_send(Write::Probe(probe)).is_err() {
            self.health.lock().unwrap().dropped += 1;
        }
    }
    pub fn event(&self, event: Event) {
        if self.tx.try_send(Write::Event(event)).is_err() {
            self.health.lock().unwrap().dropped += 1;
        }
    }
    pub fn speed(&self, run: SpeedRun) -> Result<()> {
        if run.status.starts_with("completed") {
            run.validate_result()?;
        }
        let (tx, rx) = bounded(1);
        self.tx
            .send_timeout(Write::Speed(Box::new(run), tx), Duration::from_secs(2))?;
        rx.recv_timeout(Duration::from_secs(10))?
            .map_err(anyhow::Error::msg)
    }
    pub fn flush(&self) -> Result<()> {
        let (tx, rx) = bounded(1);
        self.tx
            .send_timeout(Write::Flush(tx), Duration::from_secs(2))?;
        rx.recv_timeout(Duration::from_secs(10))?
            .map_err(anyhow::Error::msg)
    }
    pub fn read(&self) -> Result<Connection> {
        let db = Connection::open(&self.path)?;
        db.busy_timeout(Duration::from_secs(3))?;
        Ok(db)
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let db = self.read()?;
        let mut query =
            db.prepare("SELECT id,started,ended FROM sessions ORDER BY started DESC LIMIT 50")?;
        Ok(query
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    started: r.get(1)?,
                    ended: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn observations(
        &self,
        session: &str,
        since: i64,
        limit: usize,
    ) -> Result<Vec<Observation>> {
        let db = self.read()?;
        let mut q = db.prepare(
            "SELECT data FROM probes WHERE session=? AND at_ms>=? ORDER BY at_ms DESC LIMIT ?",
        )?;
        let mut rows = q.query(params![session, since, limit as i64])?;
        let mut data = Vec::new();
        while let Some(row) = rows.next()? {
            data.push(serde_json::from_str(&row.get::<_, String>(0)?)?);
        }
        data.reverse();
        Ok(data)
    }
    pub fn speeds(&self) -> Result<Vec<SpeedRun>> {
        let db = self.read()?;
        let mut q = db.prepare("SELECT data FROM speeds ORDER BY at_ms DESC LIMIT 100")?;
        let values = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
            .iter()
            .map(|s| Ok(serde_json::from_str(s)?))
            .collect()
    }
    pub fn export(&self, session: &str, path: &Path, json: bool) -> Result<()> {
        ensure_output_safe(&self.path, path)?;
        self.flush()
            .context("Save pending measurements before exporting")?;
        let output = AtomicOutput::new(path)?;
        let mut db = self.read()?;
        // Keep probes, speeds, and events in the same read snapshot while live collection continues.
        let snapshot = db.transaction()?;
        let mut q = snapshot.prepare("SELECT data FROM probes WHERE session=? ORDER BY at_ms")?;
        let mut rows = q.query([session])?;
        let file = OpenOptions::new().write(true).open(&output.temporary)?;
        if json {
            use std::io::Write;
            let mut writer = std::io::BufWriter::new(file);
            write!(
                writer,
                "{{\"schema_version\":1,\"latency_unit\":\"ms\",\"throughput_unit\":\"Mbps\",\"probes\":["
            )?;
            let mut first = true;
            while let Some(row) = rows.next()? {
                if !first {
                    write!(writer, ",")?;
                }
                first = false;
                write!(writer, "{}", row.get::<_, String>(0)?)?;
            }
            let mut speed_query =
                snapshot.prepare("SELECT data FROM speeds WHERE session=? ORDER BY at_ms")?;
            let speed_data = speed_query
                .query_map([session], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let speeds: Vec<SpeedRun> = speed_data
                .iter()
                .map(|s| serde_json::from_str(s))
                .collect::<serde_json::Result<_>>()?;
            let mut event_query =
                snapshot.prepare("SELECT data FROM events WHERE session=? ORDER BY at_ms")?;
            let event_data = event_query
                .query_map([session], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let events: Vec<Event> = event_data
                .iter()
                .map(|s| serde_json::from_str(s))
                .collect::<serde_json::Result<_>>()?;
            write!(
                writer,
                "],\"speed_tests\":{},\"events\":{}}}",
                serde_json::to_string(&speeds)?,
                serde_json::to_string(&events)?
            )?;
            writer.flush()?;
        } else {
            let mut csv = csv::Writer::from_writer(file);
            csv.write_record([
                "utc",
                "target",
                "generation",
                "ttl",
                "destination",
                "source",
                "adapter",
                "responder",
                "outcome",
                "native_status",
                "rtt_ms",
                "timeout_ms",
            ])?;
            while let Some(row) = rows.next()? {
                let p: Observation = serde_json::from_str(&row.get::<_, String>(0)?)?;
                csv.write_record([
                    chrono::DateTime::from_timestamp_millis(p.sent_ms)
                        .unwrap_or_default()
                        .to_rfc3339(),
                    TARGET_HOSTS[p.target].into(),
                    p.generation.to_string(),
                    p.ttl.map(|v| v.to_string()).unwrap_or_default(),
                    p.network.destination.to_string(),
                    p.network.source.to_string(),
                    p.network.adapter,
                    p.responder.map(|a| a.to_string()).unwrap_or_default(),
                    format!("{:?}", p.outcome),
                    p.native_status.to_string(),
                    p.rtt_ms.map(|v| v.to_string()).unwrap_or_default(),
                    p.timeout_ms.to_string(),
                ])?;
            }
            csv.flush()?;
        }
        drop(rows);
        drop(q);
        snapshot.commit()?;
        output.publish(&self.path)
    }
    pub fn backup(&self, path: &Path) -> Result<()> {
        ensure_output_safe(&self.path, path)?;
        self.flush()
            .context("Save pending measurements before backing up")?;
        let output = AtomicOutput::new(path)?;
        let db = self.read()?;
        db.backup(
            "main",
            &output.temporary,
            None::<fn(rusqlite::backup::Progress)>,
        )?;
        output.publish(&self.path)
    }
}

/// Resolve an existing destination, or its parent when the output does not exist yet.
/// This also normalizes relative paths, Windows extended paths, junctions, and symlinks.
fn canonical_destination(path: &Path) -> Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = path.file_name().context("Choose an output file")?;
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            Ok(std::fs::canonicalize(parent)
                .context("Open output folder")?
                .join(name))
        }
        Err(error) => Err(error).context("Resolve output file"),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    value.into()
}

fn ensure_output_safe(database: &Path, destination: &Path) -> Result<()> {
    let destination = canonical_destination(destination)?;
    let resolved_database = canonical_destination(database)?;
    // Include both names in case the database itself is a symlink.
    for database in [database, resolved_database.as_path()] {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let protected = canonical_destination(&with_suffix(database, suffix))?;
            anyhow::ensure!(
                !same_path(&destination, &protected),
                "Choose another output file: the active history database and its working files cannot be overwritten"
            );
        }
    }
    Ok(())
}

/// Build a complete output beside its destination before replacing an existing file.
/// On Windows, std::fs::rename uses replace-existing semantics and keeps the old file
/// intact if replacement fails (for example, when another application has it open).
struct AtomicOutput {
    temporary: PathBuf,
    destination: PathBuf,
}
impl AtomicOutput {
    fn new(destination: &Path) -> Result<Self> {
        let name = destination.file_name().context("Choose an output file")?;
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = std::fs::canonicalize(parent).context("Open output folder")?;
        let temporary = parent.join(format!(".wanalyzer-{}.tmp", uuid::Uuid::new_v4()));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .context("Create temporary output")?;
        Ok(Self {
            temporary,
            destination: parent.join(name),
        })
    }
    fn publish(self, database: &Path) -> Result<()> {
        // Check again after writing in case a destination link changed meanwhile.
        ensure_output_safe(database, &self.destination)?;
        File::options()
            .write(true)
            .open(&self.temporary)?
            .sync_all()
            .context("Flush completed output")?;
        std::fs::rename(&self.temporary, &self.destination).context("Replace output file")?;
        Ok(())
    }
}
impl Drop for AtomicOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

fn flush_batch(db: &mut Connection, batch: &[Write]) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let tx = db.transaction()?;
    for message in batch {
        match message {
            Write::Probe(p) => {
                tx.execute("INSERT INTO probes(session,target,generation,ttl,at_ms,data) VALUES(?,?,?,?,?,?)", params![p.session,p.target as i64,p.generation as i64,p.ttl,p.sent_ms,serde_json::to_string(p)?])?;
            }
            Write::Event(e) => {
                tx.execute(
                    "INSERT INTO events(session,at_ms,data) VALUES(?,?,?)",
                    params![e.session, e.at_ms, serde_json::to_string(e)?],
                )?;
            }
            Write::Speed(r, _) => {
                tx.execute("INSERT INTO speeds(id,session,at_ms,data) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE json_extract(speeds.data,'$.status')='running'", params![r.id,r.session,r.started_ms,serde_json::to_string(r)?])?;
            }
            _ => {}
        }
    }
    tx.commit()?;
    Ok(())
}
pub fn load_settings(directory: &Path) -> Result<Settings> {
    let settings: Settings =
        serde_json::from_slice(&std::fs::read(directory.join("settings.json"))?)?;
    settings.validate()?;
    Ok(settings)
}
pub fn save_settings(directory: &Path, settings: &Settings) -> Result<()> {
    settings.validate()?;
    let database = directory.join("wanalyzer.db");
    let destination = directory.join("settings.json");
    ensure_output_safe(&database, &destination)?;
    let output = AtomicOutput::new(&destination)?;
    std::fs::write(&output.temporary, serde_json::to_vec_pretty(settings)?)?;
    output.publish(&database)
}
