use crate::{
    engine::{Command, Monitor, TargetView},
    model::*,
    storage::{Session, Store, save_settings},
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
mod chrome;
mod history;
mod providers;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

const GREEN: Color32 = Color32::from_rgb(80, 220, 170);
const RED: Color32 = Color32::from_rgb(255, 102, 120);
const MUTED: Color32 = Color32::from_rgb(151, 169, 190);
const SURFACE: Color32 = Color32::from_rgb(23, 33, 48);
const BORDER: Color32 = Color32::from_rgb(46, 63, 83);
const AMBER: Color32 = Color32::from_rgb(245, 192, 103);

struct BackgroundJob {
    label: String,
    result: crossbeam_channel::Receiver<Result<String, String>>,
}

const BLUE: Color32 = Color32::from_rgb(108, 175, 255);

pub struct App {
    brand: egui::TextureHandle,
    maximize_on_start: bool,
    parent: isize,
    pip_rects: [Option<egui::Rect>; 4],
    show_provider_page: bool,
    speed_details: Option<String>,
    speed_check: bool,
    monitor: Monitor,
    store: Store,
    directory: PathBuf,
    settings: Settings,
    settings_draft: Settings,
    settings_error: String,
    job: Option<BackgroundJob>,
    close_after_job: bool,
    show_speed_log: bool,
    speed_results_only: bool,
    message_color: Color32,
    message_at: Instant,
    selected: usize,
    selected_hop: Option<u8>,
    window_secs: i64,
    page: usize,
    samples: Vec<Observation>,
    targets: [TargetView; 4],
    refresh: Instant,
    sessions: Vec<Session>,
    history: Option<String>,
    history_end: i64,
    speeds: Vec<SpeedRun>,
    message: String,
    settings_open: bool,
    active_speed: Option<SpeedRun>,
    quit_at: Option<Instant>,
    browser: Option<crate::speed::Browser>,
    iperf: Option<crate::iperf::Task>,
    iperf_options: crate::iperf::Options,
    about_open: bool,
    notices_page: usize,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        monitor: Monitor,
        store: Store,
        directory: PathBuf,
        settings: Settings,
        smoke_secs: Option<u64>,
        startup_notice: Option<String>,
    ) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(8., 5.);
        style.spacing.button_padding = Vec2::new(10., 6.);
        style.spacing.interact_size = Vec2::new(30., 26.);
        style.visuals.panel_fill = Color32::from_rgb(13, 20, 31);
        style.visuals.window_fill = SURFACE;
        style.visuals.extreme_bg_color = Color32::from_rgb(12, 20, 31);
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., BORDER);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(31, 45, 64);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(40, 61, 83);
        style.visuals.selection.bg_fill = Color32::from_rgb(27, 89, 99);
        style.visuals.selection.stroke = Stroke::new(1., GREEN);
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.));
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/wanalyzer.png"))
            .expect("Bundled app icon");
        let brand = cc.egui_ctx.load_texture(
            "brand",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            ),
            Default::default(),
        );
        let parent = match cc.window_handle().expect("main window handle").as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd.get(),
            _ => unreachable!("Windows application"),
        };
        let mut app = Self {
            brand,
            maximize_on_start: true,
            parent,
            pip_rects: [None; 4],
            show_provider_page: false,
            speed_details: None,
            speed_check: std::env::args().any(|arg| arg == "--speed-check"),
            sessions: store.sessions().unwrap_or_default(),
            speeds: store.speeds().unwrap_or_default(),
            monitor,
            store,
            directory,
            settings_draft: settings.clone(),
            settings,
            settings_error: String::new(),
            job: None,
            close_after_job: false,
            show_speed_log: true,
            speed_results_only: false,
            message_color: AMBER,
            message_at: Instant::now(),
            selected: 1,
            selected_hop: None,
            window_secs: 600,
            page: 0,
            samples: Vec::new(),
            targets: std::array::from_fn(|_| TargetView::default()),
            refresh: Instant::now() - Duration::from_secs(10),
            history: None,
            history_end: now_ms(),
            message: startup_notice.unwrap_or_default(),
            settings_open: false,
            active_speed: None,
            browser: None,
            iperf: None,
            iperf_options: crate::iperf::Options::default(),
            about_open: false,
            notices_page: 0,
            quit_at: smoke_secs.map(|s| Instant::now() + Duration::from_secs(s)),
        };
        let args: Vec<_> = std::env::args().collect();
        if let Some(provider) = args
            .iter()
            .position(|arg| arg == "--speed-check")
            .and_then(|index| args.get(index + 1))
            .and_then(|key| crate::speed::Provider::from_key(key))
        {
            app.page = 1;
            app.quit_at = Some(Instant::now() + Duration::from_secs(330));
            app.run_speed(provider);
        }
        app
    }
    fn notify(&mut self, message: impl Into<String>, color: Color32) {
        self.message = message.into();
        self.message_color = color;
        self.message_at = Instant::now();
    }
    fn poll_job(&mut self) {
        let result = self
            .job
            .as_ref()
            .and_then(|job| match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(
                    "The background operation stopped before reporting a result.".into(),
                )),
                Err(_) => None,
            });
        if let Some(result) = result {
            self.job = None;
            match result {
                Ok(message) => self.notify(message, GREEN),
                Err(error) => {
                    self.close_after_job = false;
                    self.notify(error, RED);
                }
            }
        }
    }
    fn start_export(&mut self, path: PathBuf, format: Option<bool>) {
        if self.job.is_some() {
            return;
        }
        let store = self.store.clone();
        let session = self
            .history
            .clone()
            .unwrap_or_else(|| self.monitor.session.clone());
        let label = if format.is_none() {
            "Saving database snapshot"
        } else {
            "Exporting session"
        }
        .to_string();
        let (tx, result) = crossbeam_channel::bounded(1);
        match std::thread::Builder::new()
            .name("history-export".into())
            .spawn(move || {
                let result = match format {
                    Some(json) => store.export(&session, &path, json),
                    None => store.backup(&path),
                }
                .map(|_| format!("Saved {}", display_path(&path)))
                .map_err(|error| format!("Export failed: {error:#}"));
                let _ = tx.send(result);
            }) {
            Ok(_) => self.job = Some(BackgroundJob { label, result }),
            Err(error) => self.notify(format!("Could not start export: {error}"), RED),
        }
    }
    fn refresh_data(&mut self) {
        self.poll_speed();
        self.poll_job();
        if self.refresh.elapsed() < Duration::from_millis(500) {
            return;
        }
        if let Some(session) = &self.history {
            match self.store.observations(
                session,
                self.history_end - self.window_secs * 1000,
                120_000,
            ) {
                Ok(samples) => {
                    self.samples = samples;
                    self.targets = std::array::from_fn(|_| TargetView {
                        status: "No retained measurements".into(),
                        ..Default::default()
                    });
                    for i in 0..4 {
                        if let Some(last) = self.samples.iter().rev().find(|p| p.target == i) {
                            self.targets[i].network = Some(last.network.clone());
                            self.targets[i].generation = last.generation;
                            self.targets[i].status = "Historical segment".into();
                        }
                    }
                }
                Err(e) => {
                    self.samples.clear();
                    self.targets = std::array::from_fn(|_| TargetView {
                        status: "Unable to read session".into(),
                        ..Default::default()
                    });
                    self.notify(format!("Unable to read session: {e}"), RED);
                }
            }
        } else {
            let state = self.monitor.shared.lock().unwrap();
            self.targets = state.targets.clone();
            let cutoff = now_ms() - self.window_secs * 1000;
            self.samples = state
                .samples
                .iter()
                .filter(|p| p.sent_ms >= cutoff)
                .cloned()
                .collect();
        }
        self.refresh = Instant::now();
    }
    fn segment_samples(&self, target: usize, ttl: Option<u8>) -> Vec<&Observation> {
        let generation = if self.history.is_none() {
            self.targets[target].generation
        } else {
            self.samples
                .iter()
                .rev()
                .find(|p| p.target == target)
                .map(|p| p.generation)
                .unwrap_or(0)
        };
        let mut result: Vec<_> = self
            .samples
            .iter()
            .filter(|p| p.target == target && p.ttl == ttl && p.generation == generation)
            .collect();
        result.sort_unstable_by_key(|p| p.sequence);
        result
    }
    fn stats(samples: &[&Observation]) -> Stats {
        let mut stats = Stats::default();
        for p in samples {
            stats.add(p);
        }
        stats
    }
    fn context(&self) -> String {
        self.monitor
            .shared
            .lock()
            .unwrap()
            .targets
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}: {:?}", TARGET_NAMES[i], t.network))
            .collect::<Vec<_>>()
            .join("; ")
    }
    fn run_speed(&mut self, provider: crate::speed::Provider) {
        if self.browser.is_some() || self.iperf.is_some() || self.active_speed.is_some() {
            return;
        }
        self.show_provider_page = false;
        let run = SpeedRun {
            id: uuid::Uuid::new_v4().to_string(),
            provider: provider.name().into(),
            session: self.monitor.session.clone(),
            started_ms: now_ms(),
            finished_ms: None,
            status: "running".into(),
            download_mbps: None,
            upload_mbps: None,
            ping_ms: None,
            server: String::new(),
            result_url: String::new(),
            capture_method: if provider == crate::speed::Provider::Iperf {
                "iperf3_json_v1"
            } else {
                "experimental_provider_dom_v2"
            }
            .into(),
            start_context: self.context(),
            end_context: String::new(),
            details: SpeedDetails::default(),
        };
        match self.store.speed(run.clone()) {
            Ok(()) => {
                if provider == crate::speed::Provider::Iperf {
                    self.iperf = Some(crate::iperf::Task::start(
                        self.directory.clone(),
                        self.store.clone(),
                        run.clone(),
                        self.monitor.shared.clone(),
                        self.iperf_options.clone(),
                    ));
                } else {
                    self.browser = Some(crate::speed::start(
                        self.directory.clone(),
                        self.store.clone(),
                        run.clone(),
                        self.monitor.shared.clone(),
                        self.parent,
                        provider,
                    ));
                }
                self.active_speed = Some(run);
                self.speeds = self.store.speeds().unwrap_or_default();
                self.notify(
                    format!("{} is starting. Results are saved automatically; native monitoring continues.",provider.name()),
                    BLUE,
                );
            }
            Err(e) => self.notify(
                format!(
                    "{} could not start because its run record was not saved: {e}",
                    provider.name()
                ),
                RED,
            ),
        }
    }
    fn poll_speed(&mut self) {
        if let Some(browser) = &mut self.browser {
            browser.tick();
        }
        let result = self
            .browser
            .as_mut()
            .and_then(|b| b.take_result())
            .or_else(|| self.iperf.as_ref().and_then(|task| task.take_result()));
        let Some(result) = result else {
            return;
        };
        self.browser = None;
        self.iperf = None;
        self.show_provider_page = false;
        let run = result.run;
        if let Some(error) = result.error {
            self.notify(
                format!("{}: {error}. Monitoring continues.", run.provider),
                if run.status == "canceled" { MUTED } else { RED },
            );
        }
        if result.saved {
            if run.status == "completed" {
                self.notify(
                    format!(
                        "{} saved: Download {} Mbps, Upload {} Mbps, Ping {} ms",
                        run.provider,
                        fmt(run.download_mbps),
                        fmt(run.upload_mbps),
                        fmt(run.ping_ms)
                    ),
                    GREEN,
                );
            }
            self.active_speed = None;
            self.speeds = self.store.speeds().unwrap_or_default();
            if self.speed_check {
                self.quit_at = Some(Instant::now() + Duration::from_secs(1));
            }
        } else {
            self.active_speed = Some(run);
        }
    }
    fn cancel_speed(&mut self) {
        if let Some(browser) = &mut self.browser {
            browser.cancel();
        }
        if let Some(iperf) = &self.iperf {
            iperf.cancel();
        }
    }
    fn retry_speed_save(&mut self) {
        if let Some(run) = self.active_speed.clone() {
            match self.store.speed(run) {
                Ok(()) => {
                    self.active_speed = None;
                    self.speeds = self.store.speeds().unwrap_or_default();
                    self.notify("Speed-test result saved.", GREEN);
                }
                Err(error) => self.notify(format!("Result has not been saved: {error}"), RED),
            }
        }
    }
    fn monitoring(&mut self, ui: &mut egui::Ui) {
        let end = if self.history.is_some() {
            self.history_end
        } else {
            now_ms()
        };
        let overview_scale = self
            .samples
            .iter()
            .filter(|p| p.ttl.is_none() && p.sent_ms >= end - 60_000)
            .filter_map(|p| p.rtt_ms)
            .max()
            .unwrap_or(30)
            .max(30) as f32
            * 1.2;
        let columns = if ui.available_width() < 900. { 2 } else { 4 };
        for row in (0..4).step_by(columns) {
            ui.columns(columns, |columns| {
                for (offset, column) in columns.iter_mut().enumerate() {
                    self.target_card(column, row + offset, end, overview_scale);
                }
            });
        }
        ui.add_space(6.);
        ui.horizontal_wrapped(|ui| {
            ui.heading(TARGET_NAMES[self.selected]);
            if ui
                .selectable_label(self.selected_hop.is_none(), "Destination")
                .clicked()
            {
                self.selected_hop = None;
            }
            if let Some(ttl) = self.selected_hop {
                ui.colored_label(BLUE, format!("Hop {ttl}"));
            }
            for (label, seconds) in [("1 min", 60), ("10 min", 600), ("1 hour", 3600)] {
                if ui
                    .selectable_label(self.window_secs == seconds, label)
                    .clicked()
                {
                    self.window_secs = seconds;
                    self.refresh = Instant::now() - Duration::from_secs(2);
                }
            }
        });
        if let Some(network) = &self.targets[self.selected].network {
            ui.small(format!(
                "{} · source {} · egress next hop {} · IPv4",
                network.adapter, network.source, network.next_hop
            ));
        }
        let points = self.segment_samples(self.selected, self.selected_hop);
        let stats = Self::stats(&points);
        let metrics = [
            (
                "Average RTT",
                format!("{} ms", fmt(stats.mean())),
                BLUE,
                format!(
                    "Adjacent successful RTT variation: {} ms",
                    fmt(stats.variation())
                ),
            ),
            (
                "Reply loss",
                format!("{}%", fmt(stats.loss())),
                if stats.lost > 0 {
                    RED
                } else if stats.sent == 0 {
                    MUTED
                } else {
                    GREEN
                },
                "Missing ICMP replies divided by probes sent. Local send errors are excluded."
                    .into(),
            ),
            (
                "Received",
                format!(
                    "{} / {}",
                    count_label(stats.responses),
                    count_label(stats.sent)
                ),
                MUTED,
                "Successful replies / probes sent in the selected time window.".into(),
            ),
            (
                "Min / max",
                format!(
                    "{} / {} ms",
                    stats
                        .rtts
                        .iter()
                        .min()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "--".into()),
                    stats
                        .rtts
                        .iter()
                        .max()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "--".into())
                ),
                MUTED,
                "Minimum and maximum successful round-trip times.".into(),
            ),
            (
                "95th percentile",
                format!(
                    "{} ms",
                    stats
                        .p95()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "--".into())
                ),
                MUTED,
                "95% of successful round-trip times are at or below this value.".into(),
            ),
            (
                "Local errors",
                stats.local_errors.to_string(),
                if stats.local_errors > 0 { AMBER } else { MUTED },
                "Local send failures. These are excluded from network reply-loss percentages."
                    .into(),
            ),
        ];
        let metric_columns = if ui.available_width() >= 1050. {
            6
        } else if ui.available_width() >= 680. {
            3
        } else {
            2
        };
        for row in metrics.chunks(metric_columns) {
            ui.columns(metric_columns, |columns| {
                for (column, (label, value, color, help)) in columns.iter_mut().zip(row) {
                    metric(column, label, value.clone(), *color).on_hover_text(help);
                }
            });
        }
        let end = if self.history.is_some() {
            self.history_end
        } else {
            now_ms()
        };
        let chart_height = (ui.available_height() * 0.25).clamp(95., 145.);
        let table_height = (ui.available_height() - chart_height - 62.).max(70.);
        self.hop_table(ui, table_height);

        let points = self.segment_samples(self.selected, self.selected_hop);
        ui.columns(2, |columns| {
            plot(
                &mut columns[0],
                &points,
                end,
                self.window_secs,
                false,
                &self.speeds,
                chart_height,
            );
            plot(
                &mut columns[1],
                &points,
                end,
                self.window_secs,
                true,
                &self.speeds,
                chart_height,
            );
        });
    }
    fn target_card(&mut self, ui: &mut egui::Ui, index: usize, end: i64, scale: f32) {
        let samples = self.segment_samples(index, None);
        let stats = Self::stats(&samples);
        let selected = self.selected == index;
        let paused = self.targets[index].paused;
        let status = if paused && self.history.is_none() {
            "Paused - resume to continue sampling".to_string()
        } else {
            self.targets[index].status.clone()
        };
        let address = self.targets[index]
            .network
            .as_ref()
            .map(|network| network.destination.to_string())
            .unwrap_or_else(|| "Waiting for an address".into());
        let color = if paused || stats.sent == 0 {
            MUTED
        } else if stats.lost > 0 {
            RED
        } else if stats.local_errors > 0 {
            AMBER
        } else {
            GREEN
        };
        let mut choose = false;
        let mut pause = false;
        egui::Frame::new()
            .fill(if selected {
                Color32::from_rgb(24, 46, 59)
            } else {
                SURFACE
            })
            .stroke(Stroke::new(
                1.,
                if selected {
                    GREEN.gamma_multiply(0.75)
                } else {
                    BORDER
                },
            ))
            .corner_radius(7)
            .inner_margin(9)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    choose = ui
                        .selectable_label(selected, RichText::new(TARGET_NAMES[index]).strong())
                        .clicked();
                    if self.history.is_none() {
                        pause = ui
                            .small_button(if paused { "Resume" } else { "Pause" })
                            .clicked();
                    }
                });
                ui.add(
                    egui::Label::new(RichText::new(address).monospace().color(MUTED)).truncate(),
                );
                mini_plot(ui, &samples, end, scale);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} ms", fmt(stats.mean())))
                            .strong()
                            .size(16.)
                            .color(color),
                    );
                    ui.label(RichText::new(format!("{}% loss", fmt(stats.loss()))).color(color));
                });
                ui.add(
                    egui::Label::new(RichText::new(&status).small().color(
                        if self.targets[index].network.is_none() {
                            AMBER
                        } else {
                            MUTED
                        },
                    ))
                    .truncate(),
                )
                .on_hover_text(status);
            });
        if choose {
            self.selected = index;
            self.selected_hop = None;
        }
        if pause && let Err(error) = self.monitor.commands.try_send(Command::Pause(index)) {
            self.notify(format!("Could not change sampling: {error}"), RED);
        }
    }
    fn hop_table(&mut self, ui: &mut egui::Ui, height: f32) {
        use egui_extras::{Column, TableBuilder};
        let max = self
            .samples
            .iter()
            .filter(|p| p.target == self.selected)
            .filter_map(|p| p.ttl)
            .max()
            .unwrap_or(0);
        let rows: Vec<_> = (1..=max)
            .filter_map(|ttl| {
                let samples = self.segment_samples(self.selected, Some(ttl));
                if samples.is_empty() {
                    None
                } else {
                    Some((ttl, Self::stats(&samples)))
                }
            })
            .collect();
        ui.horizontal(|ui| {
            ui.strong("Traceroute");
            ui.small(format!(
                "{} hops · click a hop to plot its latency and loss",
                rows.len()
            ));
        });
        if rows.is_empty() {
            let text = if self.history.is_some() {
                "No retained traceroute measurements in this time window."
            } else if self.targets[self.selected].paused {
                "Traceroute is paused. Resume this target to discover its route."
            } else if self.targets[self.selected].network.is_none() {
                "Discovering the network route. Literal-IP targets continue independently of DNS."
            } else {
                "Waiting for the first traceroute sweep. Hops appear as replies arrive."
            };
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_height(height);
                ui.set_min_width(ui.available_width());
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(text).color(MUTED));
                });
            });
            return;
        }
        egui::ScrollArea::horizontal().id_salt("hop-columns").auto_shrink([false,true]).show(ui,|ui|{
        ui.set_min_width(700.);
        TableBuilder::new(ui)
            .id_salt("hop-table")
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::exact(30.))
            .column(Column::remainder().at_least(120.).clip(true))
            .columns(Column::exact(42.), 3)
            .column(Column::exact(48.))
            .columns(Column::exact(50.), 3)
            .column(Column::exact(92.))
            .max_scroll_height(height)
            .min_scrolled_height(height)
            .header(22., |mut header| {
                for title in [
                    "Hop",
                    "Responders",
                    "Sent",
                    "Recv",
                    "Lost",
                    "PL%",
                    "Min ms",
                    "Avg ms",
                    "Max ms",
                    "Local / remote",
                ] {
                    header.col(|ui| {
                        ui.strong(title).on_hover_text(match title {"PL%"=>"Hop loss is unanswered ICMP probes. Routers may rate-limit these replies while forwarding traffic normally. Compare destination loss before drawing conclusions.","Local / remote"=>"Local send errors / remote ICMP error responses. Local errors are excluded from loss denominators.",_=>title});
                    });
                }
            })
            .body(|body| {
                body.rows(19., rows.len(), |mut row| {
                    let (ttl, stats) = &rows[row.index()];
                    row.set_selected(self.selected_hop == Some(*ttl));
                    let responders = if stats.responders.is_empty() {
                        "* (no replies)".into()
                    } else {
                        stats
                            .responders
                            .iter()
                            .map(|ip| ip.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let values = [
                        ttl.to_string(),
                        responders,
                        stats.sent.to_string(),
                        stats.responses.to_string(),
                        stats.lost.to_string(),
                        format!("{}%", fmt(stats.loss())),
                        stats
                            .rtts
                            .iter()
                            .min()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "--".into()),
                        fmt(stats.mean()),
                        stats
                            .rtts
                            .iter()
                            .max()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "--".into()),
                        format!("{} / {}", stats.local_errors, stats.remote_errors),
                    ];
                    for (i, value) in values.into_iter().enumerate() {
                        row.col(|ui| {
                            let color = if i == 5 && stats.lost > 0 {
                                RED
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.add(egui::Label::new(RichText::new(&value).color(color)).truncate())
                                .on_hover_text(value);
                        });
                    }
                    if row.response().clicked() {
                        self.selected_hop = Some(*ttl);
                    }
                });
            });
        });
    }
}
impl Drop for App {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.result.recv_timeout(Duration::from_secs(10));
        }
        self.browser.take();
        self.iperf.take();
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_speed();
        self.poll_job();
        if !self.message.is_empty()
            && (self.message_color == GREEN
                || self.message_color == BLUE
                || self.message_color == MUTED)
            && self.message_at.elapsed() > Duration::from_secs(10)
        {
            self.message.clear();
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.job.is_some() && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_job = true;
            self.notify(
                "Finishing the export before closing. Monitoring continues until it is saved.",
                BLUE,
            );
        }
        if self.maximize_on_start {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            self.maximize_on_start = false;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        self.refresh_data();
        if self.close_after_job && self.job.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.pip_rects = [None; 4];
        if self.quit_at.is_some_and(|at| Instant::now() >= at) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.app_shell(root);
        self.settings_dialog(&ctx);
        self.about_dialog(&ctx);
        self.speed_details_dialog(&ctx);
        let bounds: [Option<[i32; 4]>; 4] = std::array::from_fn(|index| {
            self.pip_rects[index]
                .filter(|_| !self.settings_open && !self.about_open && self.speed_details.is_none())
                .map(|rect| {
                    let scale = ctx.pixels_per_point();
                    [
                        (rect.left() * scale).round() as i32,
                        (rect.top() * scale).round() as i32,
                        (rect.right() * scale).round() as i32,
                        (rect.bottom() * scale).round() as i32,
                    ]
                })
        });
        if let Some(browser) = &mut self.browser {
            browser.place(bounds[0]);
        }
    }
}
fn fmt(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1}"))
        .unwrap_or_else(|| "--".into())
}
fn display_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .strip_prefix(r"\\?\")
        .unwrap_or(&path.to_string_lossy())
        .to_string()
}
fn clock_label(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}
fn count_label(value: u64) -> String {
    let text = value.to_string();
    let mut result = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}
fn duration_label(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1000;
    if seconds >= 3600 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}
fn metric(ui: &mut egui::Ui, label: &str, value: String, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).small().color(MUTED));
                ui.label(RichText::new(value).strong().color(color));
            });
        })
        .response
}
fn status_badge(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(label).small().strong().color(color));
        });
}
fn speed_status(status: &str) -> (&str, Color32) {
    match status {
        "completed" | "completed_manual" => ("Saved", GREEN),
        "running" => ("Running", BLUE),
        "canceled" => ("Canceled", MUTED),
        "interrupted" => ("Interrupted", AMBER),
        "capture_failed" => ("Capture failed", RED),
        _ => ("Failed", RED),
    }
}
fn mini_plot(ui: &mut egui::Ui, samples: &[&Observation], end: i64, maximum: f32) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3., Color32::from_rgb(19, 26, 36));
    let mut previous: Option<&Observation> = None;
    let cutoff = end - 60_000;
    let point = |p: &Observation| {
        egui::pos2(
            rect.left() + (p.sent_ms - cutoff) as f32 / 60_000. * rect.width(),
            rect.bottom() - 4. - p.rtt_ms.unwrap_or(0) as f32 / maximum * (rect.height() - 8.),
        )
    };
    for p in samples.iter().copied().filter(|p| p.sent_ms >= cutoff) {
        if p.outcome == Outcome::Reply {
            if let Some(prev) = previous
                && p.sequence == prev.sequence + 1
                && p.generation == prev.generation
            {
                painter.line_segment([point(prev), point(p)], Stroke::new(1.3, GREEN));
            }
            painter.circle_filled(point(p), 1.5, GREEN);
            previous = Some(p);
        } else {
            if p.outcome.counted() {
                let x = point(p).x;
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1., RED),
                );
            }
            previous = None;
        }
    }
    response.on_hover_text(format!("Last 60 seconds · shared 0–{maximum:.0} ms scale\nRed marks missing successful endpoint echoes."));
}
fn time_label(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn plot(
    ui: &mut egui::Ui,
    samples: &[&Observation],
    end: i64,
    seconds: i64,
    loss: bool,
    speeds: &[SpeedRun],
    height: f32,
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6., Color32::from_rgb(19, 26, 36));
    let graph = egui::Rect::from_min_max(
        rect.min + Vec2::new(64., 20.),
        rect.max - Vec2::new(10., 20.),
    );
    let start = end - seconds * 1000;
    let max = if loss {
        100.
    } else {
        (samples.iter().filter_map(|p| p.rtt_ms).max().unwrap_or(30) as f32 * 1.2).max(30.)
    };
    let x = |ms: i64| {
        graph.left() + ((ms - start) as f32 / (seconds * 1000) as f32).clamp(0., 1.) * graph.width()
    };
    let y = |value: f32| graph.bottom() - (value / max).clamp(0., 1.) * graph.height();
    for run in speeds.iter().filter(|s| {
        samples.first().is_some_and(|p| p.session == s.session)
            && s.started_ms <= end
            && s.finished_ms.unwrap_or(end) >= start
    }) {
        let from = x(run.started_ms);
        let to = x(run.finished_ms.unwrap_or(end));
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(from, graph.top()),
                egui::pos2(to.max(from + 2.), graph.bottom()),
            ),
            0.,
            Color32::from_rgba_unmultiplied(125, 95, 200, 45),
        );
    }
    for step in 0..=2 {
        let value = max * step as f32 / 2.;
        painter.line_segment(
            [
                egui::pos2(graph.left(), y(value)),
                egui::pos2(graph.right(), y(value)),
            ],
            Stroke::new(1., Color32::from_gray(44)),
        );
        painter.text(
            egui::pos2(graph.left() - 6., y(value)),
            egui::Align2::RIGHT_CENTER,
            format!("{value:.0}{}", if loss { "%" } else { "ms" }),
            egui::FontId::monospace(10.),
            Color32::GRAY,
        );
    }
    painter.text(
        rect.min + Vec2::new(8., 4.),
        egui::Align2::LEFT_TOP,
        if loss {
            "PACKET LOSS · 10s buckets"
        } else {
            "ROUND-TRIP LATENCY"
        },
        egui::FontId::monospace(10.),
        Color32::LIGHT_GRAY,
    );
    if loss {
        let mut buckets = BTreeMap::<i64, (u64, u64)>::new();
        for p in samples.iter().filter(|p| p.outcome.counted()) {
            let bucket = (p.sent_ms / 10_000) * 10_000;
            let b = buckets.entry(bucket).or_default();
            b.0 += 1;
            if if p.ttl.is_some() {
                !p.outcome.is_response()
            } else {
                p.outcome != Outcome::Reply
            } {
                b.1 += 1;
            }
        }
        for (bucket, (sent, lost)) in buckets {
            let a = x(bucket);
            let b = x(bucket + 10_000);
            if lost > 0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(a, y(100. * lost as f32 / sent as f32)),
                        egui::pos2(b.max(a + 2.), graph.bottom()),
                    ),
                    0.,
                    RED,
                );
            }
        }
    } else {
        let mut previous: Option<&Observation> = None;
        for p in samples {
            if matches!(p.outcome, Outcome::Reply | Outcome::Hop)
                && let Some(rtt) = p.rtt_ms
            {
                let point = egui::pos2(x(p.sent_ms), y(rtt as f32));
                if let Some(prev) = previous
                    && (p.ttl.is_some() || p.sequence == prev.sequence + 1)
                    && p.generation == prev.generation
                    && p.sent_ms - prev.sent_ms <= 15_000
                {
                    painter.line_segment(
                        [
                            egui::pos2(x(prev.sent_ms), y(prev.rtt_ms.unwrap_or(0) as f32)),
                            point,
                        ],
                        Stroke::new(1.5, GREEN),
                    );
                }
                painter.circle_filled(point, 1.8, GREEN);
                previous = Some(p);
            } else {
                if p.outcome == Outcome::Timeout {
                    painter.line_segment(
                        [
                            egui::pos2(x(p.sent_ms), graph.top()),
                            egui::pos2(x(p.sent_ms), graph.bottom()),
                        ],
                        Stroke::new(1., Color32::from_rgba_unmultiplied(255, 102, 120, 100)),
                    );
                }
                previous = None;
            }
        }
    }
    painter.text(
        egui::pos2(graph.left(), rect.bottom() - 2.),
        egui::Align2::LEFT_BOTTOM,
        clock_label(start),
        egui::FontId::monospace(10.),
        Color32::GRAY,
    );
    painter.text(
        egui::pos2(graph.right(), rect.bottom() - 2.),
        egui::Align2::RIGHT_BOTTOM,
        clock_label(end),
        egui::FontId::monospace(10.),
        Color32::GRAY,
    );
    if let Some(pos) = response.hover_pos() {
        let ms = start
            + (((pos.x - graph.left()) / graph.width()).clamp(0., 1.) as f64
                * (seconds * 1000) as f64) as i64;
        if let Some(p) = samples.iter().min_by_key(|p| (p.sent_ms - ms).abs()) {
            response.on_hover_text(format!(
                "{}\n{:?} · {} ms · {}\nNative status {}",
                time_label(p.sent_ms),
                p.outcome,
                p.rtt_ms
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "--".into()),
                p.responder
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "No responder".into()),
                p.native_status
            ));
        }
    }
}
