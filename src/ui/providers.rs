use super::*;
use crate::speed::{PROVIDERS, Provider};

impl App {
    pub(super) fn speed_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Speed tests");
            ui.checkbox(&mut self.show_speed_log,"Show log");
            if self.browser.is_some() {
                ui.toggle_value(&mut self.show_provider_page,"Show provider page")
                    .on_hover_text("Optional view for consent, provider interaction, or troubleshooting. F6 returns to app controls.");
            }
        });
        ui.label(
            "Five test methods, one history. Every attempt is logged; one test runs at a time.",
        );
        ui.small("Loaded latency is delay while the circuit is busy. Provider latency uses its own path, separate from native ICMP monitoring.");
        ui.add_space(8.);
        let area = ui.available_rect_before_wrap();
        let mut content = area;
        if self.show_speed_log {
            let width = (area.width() * 0.29).clamp(265., 320.);
            let log = egui::Rect::from_min_size(area.min, Vec2::new(width, area.height()));
            content.min.x = log.right() + 12.;
            self.speed_log(ui, log);
        }
        if self.active_speed.is_some() && self.browser.is_none() && self.iperf.is_none() {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt("unsaved-speed")
                    .max_rect(content),
                |ui| {
                    ui.colored_label(
                        RED,
                        "A result has not been saved. Retry before starting another test.",
                    );
                    if ui.button("Retry saving result").clicked() {
                        self.retry_speed_save();
                    }
                    content.min.y = ui.cursor().top() + 8.;
                },
            );
        }
        if self.show_provider_page && self.browser.is_some() {
            self.provider_browser(ui, content);
        } else {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt("native-speed-cards")
                    .max_rect(content),
                |ui| {
                    ui.set_clip_rect(content.intersect(ui.clip_rect()));
                    egui::ScrollArea::vertical()
                        .id_salt("speed-cards-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let gap = 12.;
                            let columns = if ui.available_width() < 560. { 1 } else { 2 };
                            let iperf_height = if ui.available_width() < 600. {
                                390.
                            } else {
                                310.
                            };
                            let card = Vec2::new(
                                (ui.available_width() - gap * (columns - 1) as f32)
                                    / columns as f32,
                                246.,
                            );
                            let origin = ui.cursor().min;
                            for (index, provider) in PROVIDERS.iter().enumerate() {
                                let position = origin
                                    + Vec2::new(
                                        (index % columns) as f32 * (card.x + gap),
                                        (index / columns) as f32 * (card.y + gap),
                                    );
                                self.provider_card(
                                    ui,
                                    index,
                                    egui::Rect::from_min_size(
                                        position,
                                        if *provider == Provider::Iperf {
                                            Vec2::new(ui.available_width(), iperf_height)
                                        } else {
                                            card
                                        },
                                    ),
                                );
                            }
                            ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                                origin,
                                Vec2::new(
                                    card.x * columns as f32 + gap * (columns - 1) as f32,
                                    card.y * PROVIDERS.len().div_ceil(columns) as f32
                                        + gap * (PROVIDERS.len().div_ceil(columns) - 1) as f32
                                        + iperf_height
                                        - card.y,
                                ),
                            ));
                        });
                },
            );
        }
        ui.advance_cursor_after_rect(area);
    }

    fn speed_log(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .scope_id(egui::Id::unique("all-provider-speed-log"))
                .max_rect(rect),
            |ui| {
                ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                ui.strong("Test history");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.speed_results_only, false, "All attempts");
                    ui.selectable_value(&mut self.speed_results_only, true, "Saved results");
                });
                ui.small("Click a result for full details.");
                egui::ScrollArea::vertical()
                    .id_salt("speed-history")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        let runs: Vec<_> = self
                            .speeds
                            .iter()
                            .filter(|run| {
                                !self.speed_results_only || run.status.starts_with("completed")
                            })
                            .cloned()
                            .collect();
                        if runs.is_empty() {
                            ui.add_space(12.);
                            ui.label("No tests in this view yet.");
                        }
                        for run in runs {
                            let (status, color) = speed_status(&run.status);
                            let frame = egui::Frame::new()
                                .fill(SURFACE)
                                .stroke(Stroke::new(1., BORDER))
                                .corner_radius(6)
                                .inner_margin(9)
                                .show(ui, |ui| {
                                    ui.set_width((rect.width() - 28.).max(100.));
                                    let stamp =
                                        chrono::DateTime::from_timestamp_millis(run.started_ms)
                                            .map(|t| {
                                                t.with_timezone(&chrono::Local)
                                                    .format("%m-%d %H:%M:%S")
                                                    .to_string()
                                            })
                                            .unwrap_or_default();
                                    line(ui, format!("{stamp}  {status}"), color)
                                        .on_hover_text(time_label(run.started_ms));
                                    ui.add(
                                        egui::Label::new(RichText::new(&run.provider).strong())
                                            .truncate(),
                                    );
                                    line(
                                        ui,
                                        format!(
                                            "Down {} | Up {} Mbps",
                                            fmt(run.download_mbps),
                                            fmt(run.upload_mbps)
                                        ),
                                        MUTED,
                                    );
                                    line(ui, format!("Ping {} ms", fmt(run.ping_ms)), MUTED);
                                    line(
                                        ui,
                                        format!(
                                            "DL load {}",
                                            loaded(
                                                run.details.download_latency_ms,
                                                run.details.download_increase_ms
                                            )
                                        ),
                                        MUTED,
                                    );
                                    line(
                                        ui,
                                        format!(
                                            "UL load {}",
                                            loaded(
                                                run.details.upload_latency_ms,
                                                run.details.upload_increase_ms
                                            )
                                        ),
                                        MUTED,
                                    );
                                    let note = if run.details.loaded_latency_ms.is_some() {
                                        format!(
                                            "Loaded {} ms (direction unspecified)",
                                            fmt(run.details.loaded_latency_ms)
                                        )
                                    } else if run.status == "running" {
                                        format!(
                                            "Running {}",
                                            duration_label(now_ms() - run.started_ms)
                                        )
                                    } else if !run.details.error.is_empty() {
                                        run.details.error.clone()
                                    } else if !run.server.is_empty() {
                                        run.server.clone()
                                    } else {
                                        format!(
                                            "Duration {}",
                                            duration_label(
                                                run.finished_ms.unwrap_or(run.started_ms)
                                                    - run.started_ms
                                            )
                                        )
                                    };
                                    line(ui, note, MUTED);
                                });
                            let response = ui.interact(
                                frame.response.rect,
                                ui.make_persistent_id(("result-details", &run.id)),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    ui.is_enabled(),
                                    format!("{} result, {status}. Open details", run.provider),
                                )
                            });
                            if response.clicked() {
                                self.speed_details = Some(run.id.clone());
                            }
                            response
                                .on_hover_text("Open all measurements and captured provider text");
                            ui.add_space(4.);
                        }
                    });
            },
        );
    }

    fn provider_card(&mut self, ui: &mut egui::Ui, index: usize, rect: egui::Rect) {
        let provider = PROVIDERS[index];
        let active = (provider == Provider::Iperf && self.iperf.is_some())
            || self
                .browser
                .as_ref()
                .is_some_and(|browser| browser.provider() == provider);
        let saving = active
            && (self.iperf.as_ref().is_some_and(|task| task.is_saving())
                || self
                    .browser
                    .as_ref()
                    .is_some_and(|browser| browser.is_saving()));
        let latest = self
            .speeds
            .iter()
            .find(|run| run.provider == provider.name())
            .cloned();
        let completed = latest
            .as_ref()
            .filter(|run| run.status.starts_with("completed"));
        let phase = if active && provider == Provider::Iperf {
            self.iperf
                .as_ref()
                .map(|task| task.phase())
                .unwrap_or_default()
        } else if active {
            self.browser
                .as_ref()
                .map(|browser| browser.phase().to_string())
                .unwrap_or_default()
        } else {
            latest
                .as_ref()
                .map(|run| run.details.error.clone())
                .unwrap_or_default()
        };
        let (status, color) = if saving {
            ("Saving", BLUE)
        } else if active {
            ("Running", BLUE)
        } else if let Some(run) = &latest {
            speed_status(&run.status)
        } else {
            ("Ready", MUTED)
        };
        ui.painter().rect_filled(rect, 7., SURFACE);
        ui.painter()
            .rect_stroke(rect, 7., Stroke::new(1., BORDER), egui::StrokeKind::Inside);
        ui.scope_builder(
            egui::UiBuilder::new()
                .scope_id(egui::Id::unique(("provider-card", index)))
                .max_rect(rect.shrink(11.)),
            |ui| {
                ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                ui.horizontal_wrapped(|ui| {
                    ui.strong(provider.name());
                    status_badge(ui, status, color);
                });
                ui.horizontal(|ui| {
                    if active {
                        if ui
                            .add_enabled(!saving, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            self.cancel_speed();
                        }
                        ui.spinner();
                    } else if ui
                        .add_enabled(
                            self.browser.is_none() && self.iperf.is_none() && self.active_speed.is_none(),
                            egui::Button::new("Run"),
                        )
                        .clicked()
                    {
                        self.run_speed(provider);
                    }
                    if let Some(run) = &latest
                        && ui.button("Details").clicked()
                    {
                        self.speed_details = Some(run.id.clone());
                    }
                });
                if provider == Provider::Iperf {
                    ui.add_enabled_ui(!active && self.active_speed.is_none(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut self.iperf_options.auto_tune, "Auto tune 8–128 streams");
                            if !self.iperf_options.auto_tune {
                                ui.label("Streams");
                                ui.add(egui::DragValue::new(&mut self.iperf_options.streams).range(1..=128));
                            }
                            ui.label("Seconds per direction");
                            ui.add(egui::DragValue::new(&mut self.iperf_options.seconds).range(10..=60));
                        });
                    });
                }
                line(
                    ui,
                    if phase.is_empty() {
                        match provider {
                            Provider::Devina => "Speed and latency under download/upload load",
                            Provider::Fast => "Netflix CDN speed and loaded latency",
                            Provider::Iperf => "Nearest responsive Clouvider location | automatic port selection",
                            _ => "Download, upload and provider latency",
                        }
                        .into()
                    } else {
                        phase
                    },
                    MUTED,
                );
                ui.add_space(5.);
                ui.columns(2, |columns| {
                    columns[0].small("Download");
                    columns[0].label(
                        RichText::new(format!(
                            "{} Mbps",
                            fmt(completed.and_then(|run| run.download_mbps))
                        ))
                        .size(19.)
                        .color(GREEN),
                    );
                    columns[1].small("Upload");
                    columns[1].label(
                        RichText::new(format!(
                            "{} Mbps",
                            fmt(completed.and_then(|run| run.upload_mbps))
                        ))
                        .size(19.)
                        .color(BLUE),
                    );
                });
                ui.separator();
                if provider == Provider::Iperf {
                    result_row(ui, "Server selection latency", format!("{} ms", fmt(completed.and_then(|run| run.ping_ms))));
                    if let Some(details) = latest.as_ref().and_then(|run| run.details.iperf.as_ref()) {
                        result_row(ui, "Measured streams", details.selected_streams.to_string());
                        line(ui, format!("Clouvider {} | {} seconds per direction", details.location, details.duration_seconds), MUTED);
                    }
                    ui.small("Uses full bandwidth and can transfer several GB. Auto tune takes about 75 seconds, longer if rechecking sustained speed.");
                    ui.small("Clouvider: 10 Gbps best effort. More streams cannot guarantee line rate. ICMP monitoring continues.");
                    return;
                }
                result_row(
                    ui,
                    if provider == Provider::WiFiman {
                        "Provider ping"
                    } else {
                        "Idle latency"
                    },
                    format!("{} ms", fmt(completed.and_then(|run| run.ping_ms))),
                );
                result_row(
                    ui,
                    "Download loaded",
                    loaded(
                        completed.and_then(|run| run.details.download_latency_ms),
                        completed.and_then(|run| run.details.download_increase_ms),
                    ),
                );
                result_row(
                    ui,
                    "Upload loaded",
                    loaded(
                        completed.and_then(|run| run.details.upload_latency_ms),
                        completed.and_then(|run| run.details.upload_increase_ms),
                    ),
                );
                if provider == Provider::Fast {
                    result_row(
                        ui,
                        "Loaded (unspecified)",
                        format!(
                            "{} ms",
                            fmt(completed.and_then(|run| run.details.loaded_latency_ms))
                        ),
                    );
                } else if let Some(run) = completed
                    && !run.details.assessment.is_empty()
                {
                    line(ui, run.details.assessment.clone(), MUTED);
                }
            },
        );
    }

    fn provider_browser(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt("provider-browser-fallback")
                .max_rect(rect),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Back to results").clicked() {
                        self.show_provider_page = false;
                    }
                    if let Some(browser) = &self.browser {
                        ui.strong(browser.provider().name());
                        ui.small("Browser zoom 25% | F6 returns focus");
                    }
                });
                let content = ui.available_rect_before_wrap().intersect(rect);
                if self.show_provider_page && content.width() > 40. && content.height() > 45. {
                    if self
                        .browser
                        .as_ref()
                        .is_some_and(|browser| matches!(browser.status(), Ok(true)))
                    {
                        self.pip_rects[0] = Some(content);
                    } else {
                        ui.spinner();
                        ui.label("Loading provider page...");
                    }
                }
            },
        );
    }

    pub(super) fn speed_details_dialog(&mut self, ctx: &egui::Context) {
        let focus_flag = egui::Id::unique("speed-details-focused");
        let Some(id) = self.speed_details.clone() else {
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
            return;
        };
        let first_frame = ctx.data_mut(|data| {
            let first = !data.get_temp::<bool>(focus_flag).unwrap_or(false);
            data.insert_temp(focus_flag, true);
            first
        });
        let Some(run) = self.speeds.iter().find(|run| run.id == id).cloned() else {
            self.speed_details = None;
            return;
        };
        let dialog=egui::Modal::new(egui::Id::unique("speed-result-details")).frame(egui::Frame::popup(&ctx.style_of(egui::Theme::Dark)).fill(SURFACE).inner_margin(18)).show(ctx,|ui|{
            ui.set_width((ctx.content_rect().width()-72.).clamp(260.,580.));
            ui.heading(format!("{} result",run.provider));ui.label(time_label(run.started_ms));
            let (status,color)=speed_status(&run.status);status_badge(ui,status,color);
            egui::ScrollArea::vertical().id_salt("result-details-scroll").max_height((ctx.content_rect().height()-190.).clamp(100.,500.)).show(ui,|ui|{
                result_row(ui,"Download",format!("{} Mbps",fmt(run.download_mbps)));
                result_row(ui,"Upload",format!("{} Mbps",fmt(run.upload_mbps)));
                result_row(ui,if run.provider=="WiFiman"{"Provider ping"}else if run.provider==crate::iperf::PROVIDER{"Server selection latency"}else{"Idle latency"},format!("{} ms",fmt(run.ping_ms)));
                result_row(ui,"Download-loaded latency",loaded(run.details.download_latency_ms,run.details.download_increase_ms));
                result_row(ui,"Upload-loaded latency",loaded(run.details.upload_latency_ms,run.details.upload_increase_ms));
                result_row(ui,"Loaded (unspecified)",format!("{} ms",fmt(run.details.loaded_latency_ms)));
                result_row(ui,"Jitter",format!("{} ms",fmt(run.details.jitter_ms)));
                ui.small("-- means not reported. Loaded values are total delay; (+...) is the increase over idle.");
                if !run.details.latency_note.is_empty(){ui.small(&run.details.latency_note);}
                if !run.details.assessment.is_empty(){ui.label(&run.details.assessment);}
                if !run.server.is_empty(){ui.label(format!("Server: {}",run.server));}
                if !run.details.error.is_empty(){ui.colored_label(RED,&run.details.error);}
                if let Some(details) = &run.details.iperf {
                    ui.separator();
                    ui.strong(format!("iPerf3 {} | {} measured streams", details.version, details.selected_streams));
                    ui.label(format!("Requested duration: {} s per direction | {}", details.duration_seconds, if details.auto_tune {"automatic stream tuning"} else {"fixed streams"}));
                    ui.collapsing("Stream trials and measured phases", |ui| {
                        for phase in &details.phases {
                            ui.label(format!("{} {} | P{} | port {} | {:.1} Mbps | {:.1} s | {} bytes", if phase.calibration {"Trial"} else {"Measured"}, phase.direction, phase.streams, phase.port, phase.receiver_mbps, phase.seconds, phase.receiver_bytes));
                            ui.small(format!("Sender retransmits: {} | CPU local {}%, remote {}%", phase.retransmits.map(|n| n.to_string()).unwrap_or_else(|| "--".into()), fmt(phase.local_cpu_percent), fmt(phase.remote_cpu_percent)));
                        }
                    });
                    ui.collapsing("Server ranking and port attempts", |ui| {
                        ui.label(&details.selection);
                        for attempt in &details.attempts { ui.label(attempt); }
                    });
                }
                ui.collapsing("Captured provider text",|ui|{ui.add(egui::Label::new(&run.details.raw_text).selectable(true).wrap());});
                ui.small(format!("Capture: {}",run.capture_method));
            });
            ui.separator();let close=ui.button("Close");if first_frame{close.request_focus();}close.clicked()
        });
        if dialog.inner || dialog.should_close() {
            self.speed_details = None;
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
        }
    }
}

fn line(ui: &mut egui::Ui, text: String, color: Color32) -> egui::Response {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    ui.add(egui::Label::new(RichText::new(single_line).small().color(color)).truncate())
        .on_hover_text(text)
}
fn loaded(value: Option<f64>, increase: Option<f64>) -> String {
    if let Some(increase) = increase {
        format!("{} ms (+{increase:.1})", fmt(value))
    } else {
        format!("{} ms", fmt(value))
    }
}
fn result_row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.columns(2, |columns| {
        columns[0].add(egui::Label::new(RichText::new(label).small().color(MUTED)).truncate());
        columns[1]
            .add(egui::Label::new(RichText::new(&value).small().strong()).truncate())
            .on_hover_text(value);
    });
}
