use crate::{
    model::*,
    platform,
    storage::{Store, Write},
};
use crossbeam_channel::{Sender, bounded};
use std::{
    collections::VecDeque,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct TargetView {
    pub network: Option<Network>,
    pub generation: u64,
    pub status: String,
    pub paused: bool,
    pub reached: Option<u8>,
}
impl Default for TargetView {
    fn default() -> Self {
        Self {
            network: None,
            generation: 0,
            status: "Discovering IPv4 route…".into(),
            paused: false,
            reached: None,
        }
    }
}
pub struct Shared {
    pub targets: [TargetView; 4],
    pub samples: VecDeque<Observation>,
    pub events: VecDeque<Event>,
    pub submitted: u64,
    pub in_flight: usize,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            targets: std::array::from_fn(|_| TargetView::default()),
            samples: VecDeque::new(),
            events: VecDeque::new(),
            submitted: 0,
            in_flight: 0,
        }
    }
}
pub enum Command {
    Pause(usize),
    Settings(Settings),
    Stop,
}
pub struct Monitor {
    pub shared: Arc<Mutex<Shared>>,
    pub commands: Sender<Command>,
    join: Option<JoinHandle<()>>,
    pub session: String,
}
struct Sweep {
    id: u64,
    next: u8,
    limit: u8,
    pending: usize,
}
struct Target {
    network: Option<Network>,
    generation: u64,
    paused: bool,
    sequence: u64,
    next_ping: Instant,
    next_trace: Instant,
    sweep: Option<Sweep>,
}
#[derive(Clone)]
struct Task {
    sample: Observation,
    wire_ttl: u8,
}

impl Monitor {
    pub fn start(store: Store, session: String, settings: Settings) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let worker_shared = shared.clone();
        let sid = session.clone();
        let (commands, rx) = bounded(64);
        let join = thread::Builder::new().name("monitor-scheduler".into()).spawn(move || {
            let (tasks_tx,tasks_rx) = bounded::<Task>(32); let (results_tx,results_rx) = bounded(64);
            let mut workers = Vec::new();
            for index in 0..32 {
                let tasks = tasks_rx.clone(); let results = results_tx.clone();
                workers.push(thread::Builder::new().name(format!("icmp-{index}")).spawn(move || {
                    let icmp = platform::Icmp::new();
                    while let Ok(task) = tasks.recv() {
                        let mut sample = task.sample; sample.sent_ms = now_ms();
                        let (outcome,status,responder,rtt) = match &icmp { Ok(icmp) => icmp.probe(&sample.network,task.wire_ttl,sample.timeout_ms), Err(_) => (Outcome::LocalError, 6, None, None) };
                        sample.finished_ms = now_ms(); sample.outcome = outcome; sample.native_status = status; sample.responder = responder; sample.rtt_ms = rtt;
                        if results.send(sample).is_err() { break; }
                    }
                }).unwrap());
            }
            drop(tasks_rx); drop(results_tx);
            let (resolution_tx,resolution_rx) = bounded(8); let (resolver_stop, resolver_stopped) = bounded::<()>(1);
            let resolver = thread::Builder::new().name("route-dns-observer".into()).spawn(move || {
                let mut google = None; let mut last_dns = Instant::now() - Duration::from_secs(300);
                let mut previous_route = None;
                loop {
                    let default_route = platform::gateway().ok();
                    if default_route != previous_route { last_dns = Instant::now() - Duration::from_secs(300); previous_route = default_route; }
                    if last_dns.elapsed() >= Duration::from_secs(300) || google.is_none() { if let Ok(ip) = platform::resolve_google() { google = Some(ip); } last_dns = Instant::now(); }
                    for (target,ip) in [google,Some(Ipv4Addr::new(8,8,8,8)),Some(Ipv4Addr::new(1,1,1,1)),default_route].into_iter().enumerate() {
                        let resolved = match ip { Some(ip) => platform::route(ip).map_err(|e| e.to_string()), None => Err(if target == 0 { "DNS: no IPv4 answer for google.com".into() } else { platform::gateway().err().map(|e|e.to_string()).unwrap_or_else(||"No pingable gateway".into()) }) };
                        if resolution_tx.send((target,resolved)).is_err() { return; }
                    }
                    if resolver_stopped.recv_timeout(Duration::from_secs(5)).is_ok() { return; }
                }
            }).unwrap();
            let now = Instant::now(); let mut settings = settings; let mut targets: [Target;4] = std::array::from_fn(|_|Target { network: None, generation: 0, paused: false, sequence: 0, next_ping: now, next_trace: now, sweep: None });
            // Literal addresses get their routes before DNS resolution has a chance to block.
            for (i,ip) in [(1,Ipv4Addr::new(8,8,8,8)),(2,Ipv4Addr::new(1,1,1,1))] { if let Ok(network) = platform::route(ip) { targets[i].network = Some(network.clone()); worker_shared.lock().unwrap().targets[i].network = Some(network); } }
            if let Ok(ip) = platform::gateway() && let Ok(network) = platform::route(ip) { targets[3].network = Some(network.clone()); worker_shared.lock().unwrap().targets[3].network = Some(network); }
            let mut next_submit = now; let mut active = 0usize; let mut sweep_id = 0; let mut cursor = 0; let mut previous_tick = now;
            let mut stopping = false;
            loop {
                let now = Instant::now();
                if now.duration_since(previous_tick) > Duration::from_secs(5) {
                    emit(&store,&worker_shared,&sid,None,"monitoring_gap","Scheduler/suspend gap; unsent intervals are not packet loss".into());
                    for target in &mut targets { target.sequence += 1; target.next_ping = now; target.next_trace = now; }
                    next_submit = now;
                }
                previous_tick = now;
                while let Ok(command) = rx.try_recv() { match command {
                    Command::Stop => stopping = true,
                    Command::Settings(new) => { settings = new; for t in &mut targets { t.sequence += 1; t.sweep = None; t.next_trace = now; } emit(&store,&worker_shared,&sid,None,"settings","Sampling settings changed".into()); },
                    Command::Pause(i) => { targets[i].paused = !targets[i].paused; targets[i].sequence += 1; targets[i].next_ping = now; targets[i].next_trace = now; worker_shared.lock().unwrap().targets[i].paused = targets[i].paused; emit(&store,&worker_shared,&sid,Some(i),"pause",(if targets[i].paused {"Paused"} else {"Resumed"}).to_string()); },
                } }
                while let Ok((i,resolution)) = resolution_rx.try_recv() {
                    match resolution {
                        Ok(network) => {
                            if targets[i].network.as_ref() != Some(&network) {
                                targets[i].generation += 1; targets[i].network = Some(network.clone()); targets[i].sweep = None; targets[i].sequence += 1; targets[i].next_ping = now; targets[i].next_trace = now;
                                let mut state = worker_shared.lock().unwrap(); let view = &mut state.targets[i]; view.network = Some(network.clone()); view.generation = targets[i].generation; view.reached = None; drop(state);
                                emit(&store,&worker_shared,&sid,Some(i),"network",format!("{} via {} ({})", network.destination,network.adapter,network.source));
                            }
                        },
                        Err(error) => {
                            worker_shared.lock().unwrap().targets[i].status = error.clone();
                            if targets[i].network.take().is_some() { targets[i].generation += 1; targets[i].sweep = None; let mut state = worker_shared.lock().unwrap(); state.targets[i].network = None; state.targets[i].generation = targets[i].generation; drop(state); emit(&store,&worker_shared,&sid,Some(i),"unavailable",error); }
                        }
                    }
                }
                while let Ok(sample) = results_rx.try_recv() {
                    active = active.saturating_sub(1);
                    let target = &mut targets[sample.target];
                    if sample.generation == target.generation {
                        if let Some(sweep) = &mut target.sweep && let Some(ttl) = sample.ttl && sample.sweep == sweep.id { sweep.pending = sweep.pending.saturating_sub(1); if sample.outcome == Outcome::Reply { sweep.limit = sweep.limit.min(ttl); worker_shared.lock().unwrap().targets[sample.target].reached = Some(sweep.limit); } }
                        if sample.ttl.is_none() { worker_shared.lock().unwrap().targets[sample.target].status = format!("{:?}{}",sample.outcome, sample.rtt_ms.map(|v|format!(" · {v} ms")).unwrap_or_default()); }
                    }
                    store.probe(sample.clone()); let mut state = worker_shared.lock().unwrap(); state.samples.push_back(sample); while state.samples.len() > 120_000 { state.samples.pop_front(); } state.in_flight = active;
                }
                if stopping { break; }
                for target in &mut targets { if target.sweep.as_ref().is_some_and(|s| s.next > s.limit && s.pending == 0) { target.sweep = None; } }
                if now >= next_submit && active < 32 {
                    let endpoint = (0..4).map(|n|(cursor+n)%4).find(|&i| !targets[i].paused && targets[i].network.is_some() && now >= targets[i].next_ping);
                    let candidate = endpoint.or_else(|| (0..4).map(|n|(cursor+n)%4).find(|&i| {
                        let t = &targets[i]; !t.paused && t.network.is_some() && (t.sweep.as_ref().is_some_and(|s|s.next <= s.limit) || t.sweep.is_none() && now >= t.next_trace)
                    }));
                    if let Some(i) = candidate {
                        cursor = (i+1)%4; let target = &mut targets[i];
                        let (ttl,sequence,sweep) = if endpoint.is_some() {
                            if now.duration_since(target.next_ping) > Duration::from_millis(settings.ping_ms) { target.sequence += 1; }
                            target.sequence += 1; target.next_ping = now + Duration::from_millis(settings.ping_ms); (None,target.sequence,0)
                        } else {
                            if target.sweep.is_none() { sweep_id += 1; target.next_trace = now + Duration::from_millis(settings.trace_ms); target.sweep = Some(Sweep { id: sweep_id, next: 1, limit: settings.max_hops, pending: 0 }); }
                            let s = target.sweep.as_mut().unwrap(); let ttl = s.next; s.next += 1; s.pending += 1; (Some(ttl),s.id,s.id)
                        };
                        let sample = Observation { session: sid.clone(), target: i, generation: target.generation, sequence, sweep, sent_ms: now_ms(), finished_ms: 0, ttl, network: target.network.clone().unwrap(), responder: None, outcome: Outcome::LocalError, native_status: 0, rtt_ms: None, timeout_ms: settings.timeout_ms };
                        if tasks_tx.try_send(Task { sample, wire_ttl: ttl.unwrap_or(128) }).is_ok() { active += 1; let mut state = worker_shared.lock().unwrap(); state.submitted += 1; state.in_flight = active; }
                        next_submit = now + Duration::from_millis(25);
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
            drop(tasks_tx);
            // Drain up to the configured timeout while workers finish synchronous native calls.
            for sample in results_rx.iter() { store.probe(sample); }
            for worker in workers { let _ = worker.join(); }
            let _ = resolver_stop.send(()); drop(resolution_rx);
            // DNS can be OS-blocked. Don't block application exit joining the resolver.
            drop(resolver);
            emit(&store,&worker_shared,&sid,None,"session_end","Monitoring stopped".into());
            let (ack_tx,ack_rx)=bounded(1);if store.tx.send(Write::Close(ack_tx)).is_ok(){let _=ack_rx.recv_timeout(Duration::from_secs(10));}
        }).unwrap();
        Self {
            shared,
            commands,
            join: Some(join),
            session,
        }
    }
    pub fn stop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = self.commands.send(Command::Stop);
            let _ = join.join();
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop();
    }
}
fn emit(
    store: &Store,
    shared: &Arc<Mutex<Shared>>,
    session: &str,
    target: Option<usize>,
    kind: &str,
    message: String,
) {
    let event = Event {
        at_ms: now_ms(),
        session: session.into(),
        target,
        kind: kind.into(),
        message,
    };
    store.event(event.clone());
    let mut state = shared.lock().unwrap();
    state.events.push_back(event);
    while state.events.len() > 300 {
        state.events.pop_front();
    }
}
