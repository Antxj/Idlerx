//! Top bar: navigation, address, zoom, sound, layout and grid shape.

use super::*;

impl App {
    // ------------------------------------------------------------ toolbar --

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let selected = self.cfg.selected.clone();
        let account = selected
            .as_deref()
            .and_then(|id| self.cfg.account(id))
            .cloned();
        let panel_open = selected
            .as_deref()
            .is_some_and(|id| self.panels.contains_key(id));

        // address bar follows the selected account (unless being edited)
        let editing = ui
            .ctx()
            .memory(|m| m.focused() == Some(egui::Id::new("address")));
        if !editing {
            let current = selected
                .as_deref()
                .and_then(|id| self.panels.get(id).and_then(Panel::url))
                .or_else(|| account.as_ref().map(|a| a.start_url().to_string()))
                .unwrap_or_default();
            if self.address_owner != selected || self.address != current {
                self.address = current;
                self.address_owner = selected.clone();
                self.address_error = false;
            }
        }

        ui.horizontal_centered(|ui| {
            let p = selected.as_deref().and_then(|id| self.panels.get(id));
            let nav = |ui: &mut egui::Ui, icon: &str, hint: &str, enabled: bool| {
                ui.add_enabled(
                    enabled,
                    egui::Button::new(RichText::new(icon).size(15.0)).frame(false),
                )
                .bar_tip(hint)
                .clicked()
            };
            if nav(
                ui,
                "⬅",
                tr!("Voltar", "Back"),
                p.is_some_and(Panel::can_go_back),
            ) {
                if let Some(p) = p {
                    p.back();
                }
            }
            if nav(
                ui,
                "➡",
                tr!("Avançar", "Forward"),
                p.is_some_and(Panel::can_go_forward),
            ) {
                if let Some(p) = p {
                    p.forward();
                }
            }
            if nav(ui, "⟳", tr!("Recarregar", "Reload"), p.is_some()) {
                if let Some(p) = p {
                    p.reload();
                }
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new("⛶").frame(false))
                    .bar_tip(tr!(
                        "Modo foco: só os jogos na tela (F11)",
                        "Focus mode: only the games on screen (F11)"
                    ))
                    .clicked()
                {
                    self.focus_mode = true;
                }
                self.layout_switch(ui, actions);

                if let Some(n) = update::available() {
                    let t = RichText::new(trf!("⬆ Versão {n}", "⬆ Version {n}")).color(ACCENT);
                    if ui
                        .link(t)
                        .bar_tip(tr!("Nova versão disponível", "New version available"))
                        .clicked()
                    {
                        self.dialog = Some(Dialog::Update);
                    }
                }
                ui.separator();

                // sound and zoom of the selected account
                if let Some(a) = &account {
                    let id = a.id.clone();
                    let icon = if a.muted { "🔇" } else { "🔊" };
                    let hint = if a.muted {
                        tr!("Ativar som desta conta", "Unmute this account")
                    } else {
                        tr!("Silenciar esta conta", "Mute this account")
                    };
                    if ui
                        .add(egui::Button::new(icon).frame(false))
                        .bar_tip(hint)
                        .clicked()
                    {
                        actions.push(Action::ToggleMute(id.clone()));
                    }
                    if panel_open {
                        let zoom = a.zoom;
                        let set = |z: f64, me: &Self| {
                            if let Some(p) = me.panels.get(&id) {
                                // steps of 5%
                                p.set_zoom((z * 20.0).round() / 20.0);
                            }
                        };
                        if ui
                            .add(egui::Button::new("+").frame(false))
                            .bar_tip(tr!("Aumentar zoom", "Zoom in"))
                            .clicked()
                        {
                            set(zoom + 0.05, self);
                            self.last_poll = Instant::now() - POLL;
                        }
                        if ui
                            .add(
                                egui::Button::new(format!("{:.0}%", zoom * 100.0))
                                    .frame(false)
                                    .min_size(Vec2::new(44.0, 0.0)),
                            )
                            .bar_tip(tr!(
                                "Zoom desta conta (clique para 100%)",
                                "Zoom of this account (click for 100%)"
                            ))
                            .clicked()
                        {
                            set(1.0, self);
                            self.last_poll = Instant::now() - POLL;
                        }
                        if ui
                            .add(egui::Button::new("−").frame(false))
                            .bar_tip(tr!("Diminuir zoom", "Zoom out"))
                            .clicked()
                        {
                            set(zoom - 0.05, self);
                            self.last_poll = Instant::now() - POLL;
                        }
                    }
                }

                // address field takes the remaining space
                let mut edit = egui::TextEdit::singleline(&mut self.address)
                    .id(egui::Id::new("address"))
                    .hint_text(tr!("Digite o endereço do jogo", "Type the game address"))
                    .desired_width(ui.available_width())
                    .margin(Margin::symmetric(8, 5));
                if self.address_error {
                    edit = edit.text_color(DANGER);
                }
                let enabled = account.is_some();
                let r = ui.add_enabled(enabled, edit);
                if r.changed() {
                    self.address_error = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    if let (Some(id), Some(url)) =
                        (selected.clone(), config::normalize_url(&self.address))
                    {
                        self.address = url.clone();
                        if let Some(p) = self.panels.get(&id) {
                            p.load(&url);
                            p.focus();
                        } else if let Some(a) = self.cfg.account_mut(&id) {
                            a.last_url = Some(url);
                            actions.push(Action::Open(id));
                        }
                    } else {
                        self.address_error = true;
                    }
                }
            });
        });
    }

    pub(super) fn layout_switch(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let before = (self.cfg.layout, self.cfg.grid_cols, self.cfg.grid_rows);
        if self.cfg.layout == Layout::Grid {
            let label = shape_label(self.cfg.grid_cols, self.cfg.grid_rows);
            let group = self
                .cfg
                .grid_group
                .as_deref()
                .and_then(|g| self.cfg.group(g))
                .map(|g| format!(" · {}", g.name))
                .unwrap_or_default();
            let button = ui
                .menu_button(format!("{label}{group} ⏷"), |ui| {
                    self.grid_menu(ui, actions)
                })
                .response;
            if self.screenshot.as_deref() == Some("grid-menu") {
                // documentation screenshots: the menu shown as if clicked
                egui::Area::new(egui::Id::new("grid_menu_shot"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(button.rect.left_bottom() + Vec2::new(-230.0, 4.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| self.grid_menu(ui, actions));
                    });
            }
            button.bar_tip(tr!("Formato da grade", "Grid shape"));

            // open accounts that do not fit in a fixed grid
            let ids = self.cfg.grid_accounts(|id| self.is_open_or_opening(id));
            let capacity = (self.cfg.grid_cols as usize) * (self.cfg.grid_rows as usize);
            if capacity > 0 && ids.len() > capacity {
                let names: Vec<&str> = ids[capacity..]
                    .iter()
                    .filter_map(|id| self.cfg.account(id).map(|a| a.name.as_str()))
                    .collect();
                ui.label(
                    RichText::new(format!("+{}", ids.len() - capacity))
                        .small()
                        .color(FAVORITE),
                )
                .bar_tip(trf!(
                    "Abertas, mas fora da grade (continuam rodando):\n{}\n\nArraste uma delas para um quadro para trocar.",
                    "Open but outside the grid (still running):\n{}\n\nDrag one onto a cell to swap.",
                    names.join("\n")
                ));
            }
        }
        ui.selectable_value(&mut self.cfg.layout, Layout::Grid, tr!("⊞ Grade", "⊞ Grid"))
            .bar_tip(tr!(
                "Várias contas abertas lado a lado",
                "Several open accounts side by side"
            ));
        ui.selectable_value(&mut self.cfg.layout, Layout::Tabs, tr!("▣ Uma", "▣ Single"))
            .bar_tip(tr!("Uma conta por vez", "One account at a time"));
        if before != (self.cfg.layout, self.cfg.grid_cols, self.cfg.grid_rows) {
            self.mark_dirty();
        }
    }

    pub(super) fn grid_menu(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let dark = ui.visuals().dark_mode;
        ui.set_min_width(260.0);
        ui.label(
            RichText::new(tr!("Formato", "Shape"))
                .small()
                .color(weak(dark)),
        );
        const PRESETS: [(u8, u8); 8] = [
            (0, 0),
            (2, 1),
            (1, 2),
            (3, 1),
            (2, 2),
            (3, 2),
            (3, 3),
            (4, 2),
        ];
        egui::Grid::new("grid_presets")
            .spacing([6.0, 6.0])
            .show(ui, |ui| {
                for (i, (c, r)) in PRESETS.into_iter().enumerate() {
                    let selected = self.cfg.grid_cols == c && self.cfg.grid_rows == r;
                    if shape_button(ui, c, r, selected).clicked() {
                        self.cfg.grid_cols = c;
                        self.cfg.grid_rows = r;
                        self.mark_dirty();
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new(tr!("Personalizado", "Custom"))
                .small()
                .color(weak(dark)),
        );
        ui.horizontal(|ui| {
            let auto = |v: f64, _| {
                if v == 0.0 {
                    tr!("auto", "auto").to_string()
                } else {
                    format!("{v}")
                }
            };
            ui.label(tr!("Colunas", "Columns"));
            let c = ui.add(
                egui::DragValue::new(&mut self.cfg.grid_cols)
                    .range(0..=8)
                    .custom_formatter(auto),
            );
            ui.label(tr!("Linhas", "Rows"));
            let r = ui.add(
                egui::DragValue::new(&mut self.cfg.grid_rows)
                    .range(0..=8)
                    .custom_formatter(auto),
            );
            if c.changed() || r.changed() {
                // rows only make sense with fixed columns
                if self.cfg.grid_cols == 0 {
                    self.cfg.grid_rows = 0;
                }
                self.mark_dirty();
            }
        });
        ui.label(
            RichText::new(tr!(
                "Com linhas fixas, as contas que não couberem continuam rodando fora da tela.",
                "With fixed rows, accounts that do not fit keep running off screen."
            ))
            .small()
            .color(weak(dark)),
        );

        if !self.cfg.groups.is_empty() {
            ui.separator();
            ui.label(
                RichText::new(tr!("Mostrar na grade", "Show in the grid"))
                    .small()
                    .color(weak(dark)),
            );
            let current = self.cfg.grid_group.clone();
            if ui
                .selectable_label(current.is_none(), tr!("Todas as contas", "All accounts"))
                .clicked()
            {
                actions.push(Action::GridGroup(None));
            }
            for g in &self.cfg.groups {
                if ui
                    .selectable_label(current.as_deref() == Some(g.id.as_str()), &g.name)
                    .clicked()
                {
                    actions.push(Action::GridGroup(Some(g.id.clone())));
                }
            }
        }
    }
}
