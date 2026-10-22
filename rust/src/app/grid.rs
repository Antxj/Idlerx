//! Content area: one account, the grid, placeholders and cell headers.

use super::*;

impl App {
    // ------------------------------------------------------------ content --

    pub(super) fn content(
        &mut self,
        ui: &mut egui::Ui,
        placements: &mut HashMap<String, Rect>,
        actions: &mut Vec<Action>,
    ) {
        let rect = ui.max_rect();
        let dark = ui.visuals().dark_mode;
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, content_bg(dark));

        if let Err(e) = &self.runtime {
            let e = e.clone();
            let installing = self
                .firefox_install
                .as_ref()
                .map(|r| r.lock().unwrap().clone());
            centered(ui, rect, |ui| {
                ui.label(RichText::new("⚠").size(36.0).color(FAVORITE));
                ui.label(
                    RichText::new(tr!("O Idlerx precisa do Firefox", "Idlerx needs Firefox"))
                        .size(18.0)
                        .strong(),
                );
                ui.label(tr!(
                    "As contas abrem no Firefox, cada uma num contêiner isolado, com o uBlock \
                     Origin. O Idlerx usa perfis próprios: o seu Firefox de sempre não muda.",
                    "Accounts open in Firefox, each in an isolated container, with uBlock \
                     Origin. Idlerx uses profiles of its own: your everyday Firefox does not change."
                ));
                ui.label(RichText::new(e).small().color(weak(dark)));
                ui.add_space(6.0);
                match &installing {
                    Some(None) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr!(
                                "Baixando o instalador oficial da Mozilla…",
                                "Downloading Mozilla's official installer…"
                            ));
                        });
                    }
                    Some(Some(Err(err))) => {
                        ui.label(RichText::new(err).color(DANGER));
                    }
                    _ => {}
                }
                if !matches!(installing, Some(None))
                    && ui
                        .add(
                            egui::Button::new(
                                RichText::new(tr!("Instalar o Firefox", "Install Firefox"))
                                    .color(Color32::WHITE),
                            )
                            .fill(ACCENT),
                        )
                        .clicked()
                {
                    actions.push(Action::InstallFirefox);
                }
                if ui
                    .link(tr!(
                        "Ou baixe no site da Mozilla",
                        "Or download it from Mozilla's site"
                    ))
                    .clicked()
                {
                    let _ = open::that("https://www.mozilla.org/firefox/");
                }
            });
            return;
        }

        if self.cfg.accounts.is_empty() {
            centered(ui, rect, |ui| {
                ui.label(RichText::new("⊞").size(44.0).color(ACCENT));
                ui.label(
                    RichText::new(tr!("Bem-vindo ao Idlerx", "Welcome to Idlerx"))
                        .size(20.0)
                        .strong(),
                );
                ui.label(tr!(
                    "Cada conta abre isolada (login, cookies e cache próprios). \
                     Abra várias do mesmo jogo e veja todas lado a lado.",
                    "Each account opens isolated (its own login, cookies and cache). \
                     Open several of the same game and see them side by side."
                ));
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(tr!(
                                "+ Adicionar a primeira conta",
                                "+ Add the first account"
                            ))
                            .color(Color32::WHITE),
                        )
                        .fill(ACCENT),
                    )
                    .clicked()
                {
                    actions.push(Action::NewAccount(None));
                }
            });
            return;
        }

        match self.cfg.layout {
            Layout::Tabs => {
                let Some(a) = self.selected_account().cloned() else {
                    return;
                };
                if self.panels.contains_key(&a.id) {
                    let page = self.login_bar(ui, rect, &a.id, actions);
                    placements.insert(a.id.clone(), page);
                } else {
                    self.placeholder(ui, rect, &a, actions);
                }
            }
            Layout::Grid => self.grid(ui, rect, placements, actions),
        }
    }

    /// Area of a closed, opening or failed account.
    pub(super) fn placeholder(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        a: &Account,
        actions: &mut Vec<Action>,
    ) {
        let opening = self.queue.iter().any(|q| q == &a.id);
        let failed = self.failed.get(&a.id).cloned();
        let dark = ui.visuals().dark_mode;
        centered(ui, rect, |ui| {
            let painter = ui.painter().clone();
            let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            avatar(&painter, r.center(), 28.0, a, false);
            ui.label(RichText::new(&a.name).size(18.0).strong());
            ui.label(RichText::new(config::domain(&a.url)).color(weak(dark)));
            ui.add_space(6.0);
            if opening {
                // waiting for the group's VPN, or the page being created
                if !self.vpn_wait(ui, &a.id, actions) {
                    ui.spinner();
                }
            } else {
                if let Some(e) = failed {
                    ui.label(
                        RichText::new(tr!(
                            "Não foi possível abrir esta conta:",
                            "Could not open this account:"
                        ))
                        .color(DANGER),
                    );
                    ui.label(RichText::new(e).small());
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(tr!("▶ Abrir", "▶ Open")).color(Color32::WHITE),
                        )
                        .fill(ACCENT),
                    )
                    .clicked()
                {
                    actions.push(Action::Open(a.id.clone()));
                }
            }
        });
    }

    pub(super) fn grid(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        placements: &mut HashMap<String, Rect>,
        actions: &mut Vec<Action>,
    ) {
        let dark = ui.visuals().dark_mode;
        let ids = self.cfg.grid_accounts(|id| self.is_open_or_opening(id));
        let fixed = self.cfg.grid_cols > 0 && self.cfg.grid_rows > 0;
        if ids.is_empty() && !fixed {
            centered(ui, rect, |ui| {
                ui.label(RichText::new("⊞").size(40.0).color(weak(dark)));
                ui.label(
                    RichText::new(tr!(
                        "Nenhuma conta aberta na grade",
                        "No open account in the grid"
                    ))
                    .size(16.0)
                    .strong(),
                );
                ui.label(
                    RichText::new(tr!(
                        "Abra contas pela lista (▶) ou arraste uma conta até aqui.",
                        "Open accounts from the list (▶) or drag an account here."
                    ))
                    .color(weak(dark)),
                );
            });
            let r = ui.interact(rect, egui::Id::new("grid_drop_empty"), Sense::hover());
            if let Some(p) = r.dnd_release_payload::<DragAccount>() {
                actions.push(Action::PlaceAfter(p.0.clone(), None));
            }
            return;
        }

        let area = rect.shrink(GRID_GAP);
        let header = if self.focus_mode { 0.0 } else { CELL_HEADER };
        let (cols, rows) =
            config::grid_shape(self.cfg.grid_cols, self.cfg.grid_rows, ids.len(), |n| {
                auto_columns(n, area.size(), header)
            });
        let cell = Vec2::new(
            (area.width() - GRID_GAP * (cols as f32 - 1.0)) / cols as f32,
            (area.height() - GRID_GAP * (rows as f32 - 1.0)) / rows as f32,
        );

        for slot in 0..cols * rows {
            let (c, r) = (slot % cols, slot / cols);
            let min = area.min
                + Vec2::new(
                    c as f32 * (cell.x + GRID_GAP),
                    r as f32 * (cell.y + GRID_GAP),
                );
            let cell_rect = Rect::from_min_size(min, cell);

            let Some(id) = ids.get(slot) else {
                // empty cell: an account dragged here joins the grid
                let drop = ui.interact(
                    cell_rect,
                    egui::Id::new(("empty_cell", slot)),
                    Sense::hover(),
                );
                let hovering = drop.dnd_hover_payload::<DragAccount>().is_some();
                if let Some(p) = drop.dnd_release_payload::<DragAccount>() {
                    actions.push(Action::PlaceAfter(p.0.clone(), ids.last().cloned()));
                }
                let stroke = if hovering {
                    Stroke::new(2.0, ACCENT)
                } else {
                    Stroke::new(1.0, border(dark))
                };
                ui.painter().rect_stroke(
                    cell_rect,
                    CornerRadius::same(6),
                    stroke,
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    cell_rect.center(),
                    Align2::CENTER_CENTER,
                    tr!(
                        "Arraste uma conta da lista para cá",
                        "Drag an account from the list here"
                    ),
                    FontId::proportional(12.0),
                    weak(dark),
                );
                continue;
            };
            let Some(a) = self.cfg.account(id).cloned() else {
                continue;
            };
            let selected = self.cfg.selected.as_deref() == Some(id.as_str());

            // drop target: another account dragged from the list takes this place
            let drop = ui.interact(cell_rect, egui::Id::new(("cell", id)), Sense::hover());
            let hovering = drop.dnd_hover_payload::<DragAccount>().is_some();
            if let Some(p) = drop.dnd_release_payload::<DragAccount>() {
                actions.push(Action::MoveTo(p.0.clone(), id.clone()));
            }

            let color = self
                .cfg
                .group_of(&a)
                .and_then(|g| self.cfg.group(g))
                .and_then(|g| g.color)
                .map(group_color);
            let stroke = if hovering {
                Stroke::new(2.0, ACCENT)
            } else if selected && ids.len() > 1 {
                Stroke::new(1.5, ACCENT)
            } else if let Some(c) = color {
                // the group's color marks its accounts in the grid
                Stroke::new(1.5, c.gamma_multiply(0.8))
            } else {
                Stroke::new(1.0, border(dark))
            };
            ui.painter()
                .rect_filled(cell_rect, CornerRadius::same(6), panel_bg(dark));
            ui.painter().rect_stroke(
                cell_rect.expand(1.0),
                CornerRadius::same(6),
                stroke,
                StrokeKind::Outside,
            );

            let page = Rect::from_min_max(cell_rect.min + Vec2::new(0.0, header), cell_rect.max);
            if header > 0.0 {
                let h = Rect::from_min_size(cell_rect.min, Vec2::new(cell_rect.width(), header));
                self.cell_header(ui, h, &a, actions);
            }
            if self.panels.contains_key(id) {
                let page = self.login_bar(ui, page, id, actions);
                placements.insert(id.clone(), page.shrink2(Vec2::new(1.0, 1.0)));
            } else {
                self.placeholder(ui, page, &a, actions);
            }
        }
    }

    pub(super) fn cell_header(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        a: &Account,
        actions: &mut Vec<Action>,
    ) {
        let dark = ui.visuals().dark_mode;
        let resp = ui.interact(rect, egui::Id::new(("cell_header", &a.id)), Sense::click());
        if resp.clicked() {
            actions.push(Action::Select(a.id.clone()));
        }
        if resp.double_clicked() {
            actions.push(Action::Expand(a.id.clone()));
        }
        let painter = ui.painter_at(rect);
        avatar(
            &painter,
            Pos2::new(rect.left() + 16.0, rect.center().y),
            10.0,
            a,
            self.panels.contains_key(&a.id),
        );

        let usage = self
            .panels
            .get(&a.id)
            .map(|p| metrics::fmt_duration(p.started.elapsed()))
            .unwrap_or_default();

        // buttons from the right
        let mut x = rect.right() - 4.0;
        let mut last = Rect::NOTHING;
        let mut button = |icon: &str, hint: &str| -> bool {
            let r = Rect::from_min_size(
                Pos2::new(x - 24.0, rect.top() + 2.0),
                Vec2::new(24.0, rect.height() - 4.0),
            );
            x -= 26.0;
            last = r;
            ui.place(r, egui::Button::new(icon).frame(false))
                .bar_tip(hint)
                .clicked()
        };
        if button("⛶", tr!("Mostrar sozinha", "Show alone")) {
            actions.push(Action::Expand(a.id.clone()));
        }
        if button("⏸", tr!("Pausar", "Pause")) {
            actions.push(Action::Pause(a.id.clone()));
        }
        if button("⟳", tr!("Recarregar", "Reload")) {
            actions.push(Action::Reload(a.id.clone()));
        }
        let mute = if a.muted { "🔇" } else { "🔊" };
        let mute_hint = if a.muted {
            tr!("Ativar som", "Unmute")
        } else {
            tr!("Silenciar", "Mute")
        };
        if button(mute, mute_hint) {
            actions.push(Action::ToggleMute(a.id.clone()));
        }
        if self.panels.contains_key(&a.id)
            && button(
                "🛡",
                tr!(
                    "uBlock Origin desta conta: liberar o site, remover um elemento…",
                    "uBlock Origin for this account: allow the site, remove an element…"
                ),
            )
        {
            actions.push(Action::BlockerPanel(a.id.clone(), last.left_bottom()));
        }

        let usage_w = if usage.is_empty() || rect.width() < 420.0 {
            0.0
        } else {
            70.0
        };
        if usage_w > 0.0 {
            painter.text(
                Pos2::new(x - 4.0, rect.center().y),
                Align2::RIGHT_CENTER,
                &usage,
                FontId::proportional(11.0),
                weak(dark),
            );
        }
        let name_left = rect.left() + 32.0;
        let max_w = (x - usage_w - 8.0 - name_left).max(20.0);
        let label = if a.favorite {
            format!("★ {}", a.name)
        } else {
            a.name.clone()
        };
        single_line(
            &painter,
            Pos2::new(name_left, rect.top() + 6.0),
            &label,
            FontId::proportional(13.0),
            ui.visuals().strong_text_color(),
            max_w,
        );
    }

    /// "Save this account's password?" above the page, after a login.
    /// Returns what is left for the page.
    pub(super) fn login_bar(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        id: &str,
        actions: &mut Vec<Action>,
    ) -> Rect {
        let Some(offer) = self.login_offers.iter().find(|o| o.account == id) else {
            return rect;
        };
        const H: f32 = 30.0;
        let dark = ui.visuals().dark_mode;
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), H));
        ui.painter()
            .rect_filled(bar, CornerRadius::ZERO, panel_bg(dark));
        let site = config::host_of(&offer.origin).to_string();
        let who = if offer.username.is_empty() {
            site.clone()
        } else {
            format!("{} · {site}", offer.username)
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(Vec2::new(8.0, 3.0)))
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        child.label(RichText::new("🔑").color(ACCENT));
        child.label(
            RichText::new(trf!(
                "Salvar a senha desta conta ({who})?",
                "Save this account's password ({who})?"
            ))
            .small(),
        );
        let account = id.to_string();
        if child
            .add(
                egui::Button::new(
                    RichText::new(tr!("Salvar", "Save"))
                        .small()
                        .color(Color32::WHITE),
                )
                .fill(ACCENT),
            )
            .clicked()
        {
            actions.push(Action::LoginChoice(account.clone(), LoginChoice::Save));
        }
        if child.small_button(tr!("Agora não", "Not now")).clicked() {
            actions.push(Action::LoginChoice(account.clone(), LoginChoice::NotNow));
        }
        if child
            .small_button(tr!("Nunca neste site", "Never on this site"))
            .clicked()
        {
            actions.push(Action::LoginChoice(account, LoginChoice::Never));
        }
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() + H), rect.max)
    }
}
