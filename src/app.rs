use std::{
    collections::BTreeSet,
    sync::mpsc::{self, Receiver},
    thread,
};

use eframe::egui;

use crate::{
    data::{LoadedConfig, TierFilter},
    firewall::{CHAIN, FirewallAction, IptablesPlan},
    map::{self, MapCamera},
    sdr::{fetch_live_config, load_cache, save_cache},
    settings::{load_allowed_pops, save_allowed_pops},
};

enum RefreshState {
    Idle,
    Loading(Receiver<Result<LoadedConfig, String>>),
}

pub struct ServerChooserApp {
    config: Option<LoadedConfig>,
    refresh: RefreshState,
    status: String,
    query: String,
    tier_filter: TierFilter,
    show_empty_pops: bool,
    highlighted: Option<String>,
    help_open: bool,
    map_camera: MapCamera,
    sudo_prompt: Option<SudoPrompt>,
}

struct SudoPrompt {
    action: FirewallAction,
    password: String,
    error: Option<String>,
}

impl ServerChooserApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            config: load_cache().ok(),
            refresh: RefreshState::Idle,
            status: "Ready".to_owned(),
            query: String::new(),
            tier_filter: TierFilter::All,
            show_empty_pops: false,
            highlighted: None,
            help_open: false,
            map_camera: MapCamera::default(),
            sudo_prompt: None,
        };
        app.restore_allowed_selection();
        app.refresh_live();
        app
    }

    fn refresh_live(&mut self) {
        if matches!(self.refresh, RefreshState::Loading(_)) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = fetch_live_config()
                .and_then(|(cfg, json)| {
                    save_cache(&json)?;
                    Ok(cfg)
                })
                .or_else(|err| {
                    load_cache().map_err(|cache_err| {
                        format!("Live refresh failed: {err}. Cache load failed: {cache_err}")
                    })
                });
            let _ = tx.send(result);
        });
        self.refresh = RefreshState::Loading(rx);
        self.status = "Refreshing current Valve SDR config...".to_owned();
    }

    fn receive_refresh(&mut self) {
        let RefreshState::Loading(rx) = &self.refresh else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(mut config)) => {
                apply_allowed_codes(&mut config, &load_allowed_pops());
                self.status = format!(
                    "Loaded revision {} from {} at {}",
                    config.revision,
                    config.source,
                    config.fetched_at.format("%H:%M:%S")
                );
                self.config = Some(config);
                self.refresh = RefreshState::Idle;
            }
            Ok(Err(err)) => {
                self.status = err;
                self.refresh = RefreshState::Idle;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Refresh worker stopped before returning data".to_owned();
                self.refresh = RefreshState::Idle;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let Some(config) = &self.config else {
            return Vec::new();
        };
        let needle = self.query.trim().to_lowercase();
        config
            .pops
            .iter()
            .enumerate()
            .filter(|(_, pop)| self.show_empty_pops || !pop.relays.is_empty())
            .filter(|(_, pop)| match self.tier_filter {
                TierFilter::All => true,
                TierFilter::ValvePrimary => pop.tier == 0,
                TierFilter::ValveAny => pop.tier <= 1,
                TierFilter::Partner => pop.tier > 1,
            })
            .filter(|(_, pop)| {
                needle.is_empty()
                    || pop.code.to_lowercase().contains(&needle)
                    || pop.desc.to_lowercase().contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    fn prepare_apply_firewall(&mut self) {
        let Some(config) = &self.config else {
            self.status = "No relay data loaded".to_owned();
            return;
        };
        if !config.live {
            self.status =
                "Refusing to apply iptables from cached data; refresh live Valve SDR data first"
                    .to_owned();
            return;
        }
        let selected: BTreeSet<_> = config
            .pops
            .iter()
            .filter(|pop| pop.selected && !pop.relays.is_empty())
            .map(|pop| pop.code.as_str())
            .collect();

        if selected.is_empty() {
            self.status = "Allow at least one relay POP before applying iptables".to_owned();
            return;
        }

        self.sudo_prompt = Some(SudoPrompt {
            action: FirewallAction::Apply(IptablesPlan::from_config(config, &selected)),
            password: String::new(),
            error: None,
        });
    }

    fn restore_allowed_selection(&mut self) {
        let allowed = load_allowed_pops();
        if let Some(config) = &mut self.config {
            apply_allowed_codes(config, &allowed);
        }
    }

    fn save_allowed_selection(&mut self) {
        let Some(config) = &self.config else {
            return;
        };
        let allowed = config
            .pops
            .iter()
            .filter(|pop| pop.selected && !pop.relays.is_empty())
            .map(|pop| pop.code.clone())
            .collect();
        if let Err(err) = save_allowed_pops(allowed) {
            self.status = format!("Failed to save allowed POPs: {err}");
        }
    }

    fn prepare_clear_firewall(&mut self) {
        self.sudo_prompt = Some(SudoPrompt {
            action: FirewallAction::Clear,
            password: String::new(),
            error: None,
        });
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Help", |ui| {
                if ui.button("How this works").clicked() {
                    self.help_open = true;
                    ui.close();
                }
            });
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.heading("CS2 Server Chooser");
            ui.separator();
            ui.label("Data");
            if ui.button("Refresh SDR data").clicked() {
                self.refresh_live();
            }
            ui.separator();
            ui.label("Allowed POPs");
            if ui.button("Allow Europe").clicked() {
                if let Some(config) = &mut self.config {
                    for pop in &mut config.pops {
                        pop.selected = (-12.0..=45.0).contains(&pop.lon)
                            && (35.0..=72.0).contains(&pop.lat)
                            && !pop.relays.is_empty();
                    }
                }
                self.save_allowed_selection();
            }
            if ui.button("Allow all").clicked() {
                if let Some(config) = &mut self.config {
                    for pop in &mut config.pops {
                        pop.selected = !pop.relays.is_empty();
                    }
                }
                self.save_allowed_selection();
            }
            if ui.button("Block all").clicked() {
                if let Some(config) = &mut self.config {
                    for pop in &mut config.pops {
                        pop.selected = false;
                    }
                }
                self.save_allowed_selection();
            }
            ui.separator();
            ui.label("Firewall");
            if ui.button("Apply rules").clicked() {
                self.prepare_apply_firewall();
            }
            if ui.button("Remove rules").clicked() {
                self.prepare_clear_firewall();
            }
        });
    }

    fn draw_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(&self.status);
            if let Some(config) = &self.config {
                ui.separator();
                ui.label(format!(
                    "{} POPs, {} selectable, {} allowed, {} relays",
                    config.pops.len(),
                    config.selectable_count(),
                    config.allowed_count(),
                    config.relay_count()
                ));
                ui.separator();
                ui.label(format!(
                    "{} allowed, {} blocked if applied",
                    config.allowed_count(),
                    config.blocked_count()
                ));
            }
        });
    }

    fn draw_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Relay POPs");
        ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Filter code, city, country"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tier_filter, TierFilter::All, "All");
            ui.selectable_value(&mut self.tier_filter, TierFilter::ValvePrimary, "Tier 0");
            ui.selectable_value(&mut self.tier_filter, TierFilter::ValveAny, "Valve");
            ui.selectable_value(&mut self.tier_filter, TierFilter::Partner, "Partner");
        });
        ui.checkbox(&mut self.show_empty_pops, "Show POPs without public relays");
        ui.separator();

        let indices = self.filtered_indices();
        let mut selection_changed = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let Some(config) = &mut self.config else {
                ui.label("No data loaded yet.");
                return;
            };
            for idx in indices {
                let pop = &mut config.pops[idx];
                let selected_text = format!(
                    "{}  {}  tier {}  {} relays",
                    pop.code.to_uppercase(),
                    pop.desc,
                    pop.tier,
                    pop.relays.len()
                );
                let response = ui
                    .horizontal(|ui| {
                        let checkbox = ui.add_enabled(
                            !pop.relays.is_empty(),
                            egui::Checkbox::new(&mut pop.selected, "Allow"),
                        );
                        if checkbox.changed() {
                            selection_changed = true;
                        }
                        ui.selectable_label(
                            self.highlighted.as_deref() == Some(&pop.code),
                            selected_text,
                        )
                    })
                    .inner;
                if response.hovered() {
                    self.highlighted = Some(pop.code.clone());
                }
                if response.clicked() && !pop.relays.is_empty() {
                    pop.selected = !pop.selected;
                    selection_changed = true;
                }
                if !pop.relays.is_empty() {
                    ui.small(format!(
                        "{} when applied. Ports {}-{}",
                        if pop.selected { "Allowed" } else { "Blocked" },
                        pop.relays[0].port_range[0],
                        pop.relays[0].port_range[1]
                    ));
                }
                ui.separator();
            }
        });
        if selection_changed {
            self.save_allowed_selection();
        }
    }

    fn draw_sudo_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.sudo_prompt else {
            return;
        };
        let mut close = false;
        let mut result = None;

        egui::Window::new(prompt.action.label())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(format!(
                    "Selected POPs are allowed; unselected POPs are blocked. This only modifies the {CHAIN} chain plus its owned OUTPUT jump."
                ));
                ui.add_space(8.0);
                ui.label("Sudo password");
                ui.add(
                    egui::TextEdit::singleline(&mut prompt.password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                if let Some(error) = &prompt.error {
                    ui.colored_label(egui::Color32::from_rgb(235, 111, 96), error);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Run").clicked() {
                        result = Some(prompt.action.run(Some(&prompt.password)));
                    }
                    if ui.button("Use cached sudo").clicked() {
                        result = Some(prompt.action.run(None));
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        if let Some(action_result) = result {
            match action_result {
                Ok(message) => {
                    self.status = message;
                    close = true;
                }
                Err(err) => {
                    if let Some(prompt) = &mut self.sudo_prompt {
                        prompt.error = Some(err);
                    }
                }
            }
        }

        if close {
            self.sudo_prompt = None;
        }
    }

    fn draw_help(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }

        egui::Window::new("Help")
            .open(&mut self.help_open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label("Checked POPs are allowed. Unchecked POPs are blocked when you apply firewall rules.");
                ui.label("The app uses Valve's current Steam Datagram Relay config for CS2, not old server IP:port matchmaking data.");
                ui.label(format!(
                    "Firewall changes are limited to the {CHAIN} chain and its owned OUTPUT jump."
                ));
                ui.separator();
                ui.label("Map controls");
                ui.label("Mouse wheel zooms the map around the cursor.");
                ui.label("Drag with left or right mouse button to move the map.");
                ui.label("Click a relay point to toggle whether that POP is allowed.");
                ui.label("Hover a row or map point to show relay details.");
            });
    }
}

impl eframe::App for ServerChooserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.highlighted = None;
        self.receive_refresh();
        if matches!(self.refresh, RefreshState::Loading(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.draw_toolbar(ui);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            self.draw_status(ui);
        });

        egui::SidePanel::right("list")
            .resizable(true)
            .default_width(390.0)
            .show(ctx, |ui| {
                self.draw_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let interaction = map::draw_world_map(
                ui,
                self.config.as_ref(),
                self.show_empty_pops,
                self.highlighted.as_deref(),
                &mut self.map_camera,
            );
            if let Some(code) = interaction.clicked {
                if let Some(config) = &mut self.config {
                    if let Some(pop) = config.pops.iter_mut().find(|pop| pop.code == code) {
                        pop.selected = !pop.selected;
                    }
                }
                self.save_allowed_selection();
            }
        });

        self.draw_sudo_prompt(ctx);
        self.draw_help(ctx);
    }
}

fn apply_allowed_codes(config: &mut LoadedConfig, allowed: &BTreeSet<String>) {
    for pop in &mut config.pops {
        pop.selected = allowed.contains(&pop.code) && !pop.relays.is_empty();
    }
}
