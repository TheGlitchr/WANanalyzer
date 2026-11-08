use super::*;

impl App {
    pub(super) fn history_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("History & export");
            if ui.button("Refresh sessions").clicked() {
                match self.store.sessions() {
                    Ok(sessions) => self.sessions = sessions,
                    Err(error) => self.notify(format!("Unable to refresh history: {error}"), RED),
                }
                match self.store.speeds() {
                    Ok(speeds) => self.speeds = speeds,
                    Err(error) => {
                        self.notify(format!("Unable to refresh speed results: {error}"), RED)
                    }
                }
            }
        });
        ui.label("Each app launch starts a session. View its retained measurements or export the full saved session.");
        ui.add_space(10.);
        ui.columns(2,|columns|{
            egui::Frame::group(columns[0].style()).fill(SURFACE).inner_margin(12).show(&mut columns[0],|ui|{
                ui.strong("Monitoring sessions");
                ui.small("Most recent 50 sessions");
                egui::ScrollArea::vertical().id_salt("sessions-list").max_height(260.).show(ui,|ui|{
                    if self.sessions.is_empty(){ui.label("Your first session will appear here shortly.");}
                    for session in &self.sessions {
                        let live=session.id==self.monitor.session;
                        let selected=if live{self.history.is_none()}else{self.history.as_ref()==Some(&session.id)};
                        egui::Frame::new().fill(if selected{Color32::from_rgb(24,46,59)}else{Color32::TRANSPARENT}).corner_radius(5).inner_margin(8).show(ui,|ui|{
                            ui.horizontal_wrapped(|ui|{
                                ui.strong(time_label(session.started));
                                if live {status_badge(ui,"Live",GREEN);} else {ui.small(duration_label(session.ended.unwrap_or(session.started)-session.started));}
                                if ui.button(if live{"View live"}else{"View session"}).on_hover_text(format!("Session {}",session.id)).clicked(){
                                    self.history=if live{None}else{Some(session.id.clone())};
                                    self.history_end=session.ended.unwrap_or_else(now_ms);
                                    self.selected_hop=None;self.page=0;self.refresh=Instant::now()-Duration::from_secs(10);
                                }
                            });
                        });
                    }
                });
            });
            egui::Frame::group(columns[1].style()).fill(SURFACE).inner_margin(12).show(&mut columns[1],|ui|{
                ui.strong("Export saved data");
                let session=self.history.as_deref().unwrap_or(&self.monitor.session);
                ui.label(if self.history.is_some(){"Selected historical session"}else{"Current live session"});
                ui.small(format!("Session {}",session.chars().take(8).collect::<String>())).on_hover_text(session);
                ui.add_space(8.);
                let enabled=self.job.is_none();
                for (label,json,extension) in [("Probe measurements (.csv)",false,"csv"),("Session + speed results (.json)",true,"json")] {
                    if ui.add_enabled(enabled,egui::Button::new(label)).on_hover_text(if json{"Exports every retained probe, speed result, and event for the selected session."}else{"Exports every retained probe for the selected session, including hop replies and loss."}).clicked()
                        && let Some(path)=rfd::FileDialog::new().add_filter(extension,&[extension]).set_file_name(format!("wanalyzer-session.{extension}")).save_file() {
                        self.start_export(path,Some(json));
                    }
                }
                ui.separator();
                if ui.add_enabled(enabled,egui::Button::new("All history snapshot (.db)")).on_hover_text("A consistent SQLite backup containing all retained sessions, probes, events, and speed results.").clicked()
                    && let Some(path)=rfd::FileDialog::new().add_filter("SQLite database",&["db"]).set_file_name("wanalyzer-backup.db").save_file(){
                    self.start_export(path,None);
                }
                if let Some(job)=&self.job {ui.horizontal(|ui|{ui.spinner();ui.label(format!("{}...",job.label));});}
                ui.add_space(8.);
                ui.small(format!("Raw measurements: {} hours | Speed results: 365 days",self.settings.raw_hours));
                ui.small("Long-range minute summaries are not available in this build.");
            });
        });
        ui.add_space(10.);
        ui.strong("Live monitoring events");
        ui.small("These events belong to the current live session, including while you view historical data.");
        let events: Vec<_> = self
            .monitor
            .shared
            .lock()
            .unwrap()
            .events
            .iter()
            .rev()
            .take(50)
            .cloned()
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("live-events")
            .max_height(160.)
            .show(ui, |ui| {
                if events.is_empty() {
                    ui.label("No monitoring events yet.");
                }
                for event in events {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(clock_label(event.at_ms))
                                .monospace()
                                .color(MUTED),
                        );
                        ui.strong(&event.kind);
                        ui.label(&event.message);
                    });
                }
            });
        ui.add_space(8.);
        ui.small(format!("Data folder: {}", display_path(&self.directory)));
    }
}
