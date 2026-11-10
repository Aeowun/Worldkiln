// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use egui::{self, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MultiplayerAction {
    Host,
    Join,
    Test,
    Stop,
    Restart,
    ViewLogs,
}

pub(crate) struct MultiplayerUi {
    pub open_test: bool,
    pub open_host: bool,
    pub open_join: bool,
    pub players: usize,
    pub arrange_windows: bool,
    pub capture_logs: bool,
    pub port: u16,
    pub join_address: String,
    pub player_limit: usize,
    pub action: Option<MultiplayerAction>,
    pub running: bool,
    pub active_players: usize,
    pub status: String,
    pub logs_path: Option<std::path::PathBuf>,
}

impl Default for MultiplayerUi {
    fn default() -> Self {
        Self {
            open_test: false,
            open_host: false,
            open_join: false,
            players: 2,
            arrange_windows: true,
            capture_logs: true,
            port: 47001,
            join_address: "127.0.0.1:47001".into(),
            player_limit: 8,
            action: None,
            running: false,
            active_players: 0,
            status: String::new(),
            logs_path: None,
        }
    }
}

impl MultiplayerUi {
    pub fn draw_play_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Host Game").clicked() {
            self.open_host = true;
            ui.close_menu();
        }
        if ui.button("Join Game").clicked() {
            self.open_join = true;
            ui.close_menu();
        }
        if ui.button("Test Multiplayer").clicked() {
            self.open_test = true;
            ui.close_menu();
        }
    }

    pub fn draw_windows(&mut self, ctx: &egui::Context) {
        if self.open_test {
            let mut open = self.open_test;
            egui::Window::new("Test Multiplayer")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.label("Run connected game instances using an isolated snapshot of your current project.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Players");
                        ui.add(egui::Slider::new(&mut self.players, 2..=8).show_value(true));
                    });
                    ui.label(RichText::new("Window layout").strong());
                    ui.radio_value(&mut self.arrange_windows, true, "Arrange windows automatically");
                    ui.radio_value(&mut self.arrange_windows, false, "Keep windows independent");
                    ui.checkbox(&mut self.capture_logs, "Capture session and player logs");
                    ui.add_space(8.0);
                    ui.label(RichText::new("Project protection").strong());
                    ui.label("Unsaved Editor changes are included in the test snapshot. Test progress and saves stay separate from your project.");
                    if !self.status.is_empty() {
                        ui.separator();
                        ui.label(&self.status);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.open_test = false;
                        }
                        if ui.add_enabled(!self.running, egui::Button::new("Start Test")).clicked() {
                            self.action = Some(MultiplayerAction::Test);
                        }
                    });
                });
            self.open_test &= open;
        }
        if self.open_host {
            let mut open = self.open_host;
            egui::Window::new("Host Game")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Port");
                        ui.add(egui::DragValue::new(&mut self.port).range(1024..=65535));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Maximum players");
                        ui.add(egui::Slider::new(&mut self.player_limit, 2..=8));
                    });
                    if ui
                        .add_enabled(!self.running, egui::Button::new("Host Game"))
                        .clicked()
                    {
                        self.action = Some(MultiplayerAction::Host);
                        self.open_host = false;
                    }
                });
            self.open_host &= open;
        }
        if self.open_join {
            let mut open = self.open_join;
            egui::Window::new("Join Game")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Server address and port");
                    ui.text_edit_singleline(&mut self.join_address);
                    if ui
                        .add_enabled(!self.running, egui::Button::new("Join Game"))
                        .clicked()
                    {
                        self.action = Some(MultiplayerAction::Join);
                        self.open_join = false;
                    }
                });
            self.open_join &= open;
        }
        if !self.running
            && !self.status.is_empty()
            && !self.open_test
            && !self.open_host
            && !self.open_join
        {
            egui::Window::new("Multiplayer")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(&self.status);
                    ui.horizontal(|ui| {
                        if ui.button("Dismiss").clicked() {
                            self.status.clear();
                        }
                        if ui
                            .add_enabled(self.logs_path.is_some(), egui::Button::new("View Logs"))
                            .clicked()
                        {
                            self.action = Some(MultiplayerAction::ViewLogs);
                        }
                    });
                });
        }
        if self.running {
            egui::Window::new("Multiplayer Session")
                .default_pos(egui::pos2(30.0, 100.0))
                .collapsible(true)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(&self.status);
                    ui.label(format!("Processes: {}", self.active_players));
                    ui.horizontal(|ui| {
                        if ui.button("Stop Test").clicked() {
                            self.action = Some(MultiplayerAction::Stop);
                        }
                        if ui.button("Restart Test").clicked() {
                            self.action = Some(MultiplayerAction::Restart);
                        }
                        if ui
                            .add_enabled(self.logs_path.is_some(), egui::Button::new("View Logs"))
                            .clicked()
                        {
                            self.action = Some(MultiplayerAction::ViewLogs);
                        }
                    });
                });
        }
    }
}
