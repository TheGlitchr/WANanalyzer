use super::*;

impl App {
    pub(super) fn open_settings(&mut self) {
        self.settings_draft = self.settings.clone();
        self.settings_error.clear();
        self.settings_open = true;
    }

    pub(super) fn app_shell(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        if !self.settings_open && !self.about_open && self.speed_details.is_none() {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
                self.open_settings();
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F1))
            {
                self.about_open = true;
            }
            for (key, page) in [
                (egui::Key::Num1, 0),
                (egui::Key::Num2, 1),
                (egui::Key::Num3, 2),
            ] {
                if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, key)) {
                    self.page = page;
                }
            }
        }
        egui::Panel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(root, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.image((self.brand.id(), Vec2::splat(27.)));
                    ui.label(RichText::new("WANanalyzer").size(20.).strong());
                    ui.add_space(14.);
                    for (page, name) in ["Monitor", "Speed tests", "History & export"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.page, page, name)
                            .on_hover_text(format!("Ctrl+{}", page + 1));
                    }
                    ui.add_space(12.);
                    if ui.button("Settings").on_hover_text("Ctrl+,").clicked() {
                        self.open_settings();
                    }
                    if ui.button("About").on_hover_text("F1").clicked() {
                        self.about_open = true;
                    }
                });
            });
        let health = self.store.health.lock().unwrap().clone();
        egui::Panel::bottom("recording-status")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(egui::Margin::symmetric(12, 5)))
            .show(root, |ui| {
                ui.horizontal_wrapped(|ui| {
                    status_badge(ui, if health.error.is_some() {"Recording error"} else {"Recording"}, if health.error.is_some() {RED} else {GREEN});
                    ui.small(format!("{} records saved", count_label(health.saved)));
                    if health.dropped > 0 {
                        ui.colored_label(RED, format!("{} unsaved", count_label(health.dropped)))
                            .on_hover_text("These measurements were not saved. Recording gaps are not counted as network packet loss.");
                    }
                    if let Some(job) = &self.job { ui.spinner(); ui.small(format!("{}...", job.label)); }
                    if ui.small_button("Data folder").on_hover_text(display_path(&self.directory)).clicked()
                        && let Err(error)=std::process::Command::new("explorer.exe").arg(&self.directory).spawn() {
                            self.notify(format!("Could not open data folder: {error}"),RED);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small("IPv4  |  Portable desktop monitor");
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 20, 31))
                    .inner_margin(12),
            )
            .show(root, |ui| {
                if let Some(error) = health.error {
                    egui::Frame::group(ui.style())
                        .fill(RED.gamma_multiply(0.1))
                        .show(ui, |ui| {
                            ui.colored_label(RED, format!("History is not recording: {error}"));
                        });
                }
                if self.history.is_some() {
                    ui.horizontal_wrapped(|ui| {
                        status_badge(ui, "Historical session", AMBER);
                        ui.label("Live monitoring continues in the background.");
                        if ui.button("Return to live").clicked() {
                            self.history = None;
                            self.selected_hop = None;
                            self.refresh = Instant::now() - Duration::from_secs(10);
                        }
                    });
                    ui.add_space(4.);
                }
                if !self.message.is_empty() {
                    egui::Frame::new()
                        .fill(self.message_color.gamma_multiply(0.1))
                        .stroke(Stroke::new(1., self.message_color.gamma_multiply(0.35)))
                        .corner_radius(5)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(self.message_color, &self.message);
                                if ui.small_button("Dismiss").clicked() {
                                    self.message.clear();
                                }
                            });
                        });
                    ui.add_space(5.);
                }
                match self.page {
                    0 => {
                        if ui.available_height() < 540. {
                            egui::ScrollArea::vertical()
                                .id_salt("compact-monitor")
                                .show(ui, |ui| {
                                    ui.set_height(if ui.available_width() < 900. {
                                        800.
                                    } else {
                                        620.
                                    });
                                    self.monitoring(ui);
                                });
                        } else {
                            self.monitoring(ui);
                        }
                    }
                    1 => self.speed_page(ui),
                    _ => {
                        egui::ScrollArea::vertical()
                            .id_salt("history-page")
                            .show(ui, |ui| self.history_page(ui));
                    }
                }
            });
    }

    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) {
        let focus_flag = egui::Id::unique("settings-modal-focused");
        if !self.settings_open {
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
            return;
        }
        let first_frame = ctx.data_mut(|data| {
            let first = !data.get_temp::<bool>(focus_flag).unwrap_or(false);
            data.insert_temp(focus_flag, true);
            first
        });
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let size = ctx.content_rect().size();
        let dialog = egui::Modal::new(egui::Id::unique("settings-modal"))
            .frame(dialog_frame(ctx))
            .backdrop_color(Color32::from_black_alpha(170))
            .show(ctx,|ui|{
                ui.set_width((size.x - 72.).clamp(260., 460.));
                ui.heading("Settings");
                ui.label(RichText::new("Adjust the display and measurement schedule.").color(MUTED));
                ui.add_space(8.);
                ui.separator();
                egui::ScrollArea::vertical().id_salt("settings-scroll")
                    .max_height((size.y - 190.).clamp(100., 500.))
                    .auto_shrink([false,true]).show(ui,|ui|{
                egui::Grid::new("settings-fields").spacing([20.,12.]).show(ui,|ui|{
                    ui.label("Interface scale");
                    ui.horizontal(|ui|{
                        let scale=ui.add(egui::Slider::new(&mut self.settings_draft.ui_scale,0.65..=1.35).step_by(0.05).show_value(false));
                        if first_frame {scale.request_focus();}
                        ui.label(format!("{:.0}%",self.settings_draft.ui_scale*100.));
                    });ui.end_row();
                    ui.label("Ping interval").on_hover_text("A successful echo measures end-to-end latency; missing successful echoes count toward endpoint loss.");
                    ui.add(egui::DragValue::new(&mut self.settings_draft.ping_ms).range(500..=60_000).speed(100.).suffix(" ms"));ui.end_row();
                    ui.label("Traceroute interval");ui.add(egui::DragValue::new(&mut self.settings_draft.trace_ms).range(5_000..=300_000).speed(1000.).suffix(" ms"));ui.end_row();
                    ui.label("Reply timeout");ui.add(egui::DragValue::new(&mut self.settings_draft.timeout_ms).range(100..=10_000).speed(100.).suffix(" ms"));ui.end_row();
                    ui.label("Maximum hops");ui.add(egui::DragValue::new(&mut self.settings_draft.max_hops).range(1..=64));ui.end_row();
                    ui.label("Raw measurement history");ui.add(egui::DragValue::new(&mut self.settings_draft.raw_hours).range(1..=168).suffix(" hours"));ui.end_row();
                });
                ui.add_space(8.);
                ui.small("Changes take effect when applied. Probing is limited to 40 submissions per second and 32 in flight; long sweeps can take longer than the requested interval.");
                if !self.settings_error.is_empty(){ui.colored_label(RED,&self.settings_error);}
                });
                ui.add_space(6.);
                ui.separator();
                ui.horizontal_wrapped(|ui|{
                    save=ui.button("Apply & save").clicked();
                    cancel=ui.button("Cancel").clicked();
                    if ui.button("Restore defaults").clicked(){self.settings_draft=Settings::default();self.settings_error.clear();}
                });
            });
        if cancel || dialog.should_close() {
            open = false;
        } else if save {
            let applied = self
                .settings_draft
                .validate()
                .map_err(|error| error.to_string())
                .and_then(|_| {
                    self.monitor
                        .commands
                        .try_send(Command::Settings(self.settings_draft.clone()))
                        .map_err(|error| format!("Unable to apply settings: {error}"))
                });
            match applied {
                Err(error) => self.settings_error = error,
                Ok(()) => {
                    self.settings = self.settings_draft.clone();
                    ctx.set_zoom_factor(self.settings.ui_scale);
                    match save_settings(&self.directory, &self.settings) {
                        Ok(()) => {
                            self.notify("Settings applied and saved.", GREEN);
                            open = false;
                        }
                        Err(error) => {
                            self.settings_error = format!(
                                "Display and sampling applied, but could not save: {error}. History retention changes require saving. Retry Apply & save."
                            )
                        }
                    }
                }
            }
        }
        if !open {
            self.settings_draft = self.settings.clone();
            self.settings_error.clear();
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
        }
        self.settings_open = open;
    }

    pub(super) fn about_dialog(&mut self, ctx: &egui::Context) {
        let focus_flag = egui::Id::unique("about-modal-focused");
        if !self.about_open {
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
            return;
        }
        let first_frame = ctx.data_mut(|data| {
            let first = !data.get_temp::<bool>(focus_flag).unwrap_or(false);
            data.insert_temp(focus_flag, true);
            first
        });
        let size = ctx.content_rect().size();
        let mut close = false;
        let dialog = egui::Modal::new(egui::Id::unique("about-modal"))
            .frame(dialog_frame(ctx))
            .backdrop_color(Color32::from_black_alpha(170))
            .show(ctx,|ui|{
            ui.set_width((size.x - 72.).clamp(260., 520.));
            ui.horizontal(|ui|{
                ui.image((self.brand.id(),Vec2::splat(44.)));
                ui.vertical(|ui|{ui.heading("About WANanalyzer");ui.label(RichText::new(format!("Version {} - development build",env!("CARGO_PKG_VERSION"))).color(MUTED));});
            });
            ui.add_space(8.);
            ui.separator();
            egui::ScrollArea::vertical().id_salt("about-scroll")
                .max_height((size.y - 190.).clamp(100., 520.))
                .auto_shrink([false,true]).show(ui,|ui|{
            ui.label("Continuous latency, traceroute, per-hop reply loss, and local history in one portable Windows app.");
            ui.label("Native speed-test cards use WiFiman, Speedtest.net, FAST.com, Devina bufferbloat, and iPerf3 with Clouvider servers. Every attempt is logged, including cancellation and failure.");
            ui.small("Provider adapters read rendered page results in a background browser. Capture is experimental and depends on provider layouts. An optional browser view handles interaction. This app is independent of the providers and has no provider endorsement.");
            ui.separator();
            ui.strong("Keyboard shortcuts");
            ui.label("Ctrl+1  Monitor     Ctrl+2  Speed tests     Ctrl+3  History");
            ui.label("Ctrl+,  Settings     F1  About     Escape  Close dialog");
            ui.label("F6  Return from a provider page to app controls");
            ui.collapsing("Third-party licenses and notices",|ui|{
                let notices=include_str!("../../assets/THIRD_PARTY_NOTICES.txt");
                let pages=notices.lines().count().div_ceil(100);
                ui.horizontal(|ui|{
                    if ui.add_enabled(self.notices_page>0,egui::Button::new("Previous")).clicked(){self.notices_page-=1;}
                    ui.label(format!("Page {} / {}",self.notices_page+1,pages));
                    if ui.add_enabled(self.notices_page+1<pages,egui::Button::new("Next")).clicked(){self.notices_page+=1;}
                });
                let text=notices.lines().skip(self.notices_page*100).take(100).collect::<Vec<_>>().join("\n");
                egui::ScrollArea::vertical().max_height(300.).show(ui,|ui|{ui.add(egui::Label::new(text).selectable(true));});
            });
            });
            ui.add_space(6.);
            ui.separator();
            let button=ui.button("Close");
            if first_frame {button.request_focus();}
            close=button.clicked();
        });
        if close || dialog.should_close() {
            self.about_open = false;
            ctx.data_mut(|data| data.remove::<bool>(focus_flag));
        }
    }
}

fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.style_of(egui::Theme::Dark))
        .fill(SURFACE)
        .stroke(Stroke::new(1., BORDER))
        .corner_radius(9)
        .inner_margin(18)
}
