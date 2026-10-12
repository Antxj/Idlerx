//! Side list: accounts, groups, drag and drop, totals.

use super::*;

impl App {
    // ------------------------------------------------------------ sidebar --

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let dark = ui.visuals().dark_mode;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Idlerx").size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let b = egui::Button::new(
                    RichText::new(tr!("+ Nova conta", "+ New account")).color(Color32::WHITE),
                )
                .fill(ACCENT);
                if ui.add(b).side_tip("Ctrl+N").clicked() {
                    actions.push(Action::NewAccount(None));
                }
            });
        });
        ui.add_space(8.0);
        // the label row is also where an account is dropped to leave its group
        let label_row = ui
            .horizontal(|ui| {
                ui.label(
                    RichText::new(trf!(
                        "CONTAS · {}",
                        "ACCOUNTS · {}",
                        self.cfg.accounts.len()
                    ))
                    .small()
                    .color(weak(dark)),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(tr!("+ Grupo", "+ Group")).small())
                                .frame(false),
                        )
                        .side_tip(tr!(
                            "Criar um grupo para organizar as contas",
                            "Create a group to organize the accounts"
                        ))
                        .clicked()
                    {
                        actions.push(Action::NewGroup(None));
                    }
                });
            })
            .response;
        let ungroup = ui.interact(
            label_row.rect,
            egui::Id::new("ungroup_drop"),
            Sense::hover(),
        );
        if let Some(p) = ungroup.dnd_hover_payload::<DragAccount>() {
            if self
                .cfg
                .account(&p.0)
                .is_some_and(|a| self.cfg.group_of(a).is_some())
            {
                ui.painter().rect_stroke(
                    label_row.rect.expand(2.0),
                    CornerRadius::same(4),
                    Stroke::new(1.5, ACCENT),
                    StrokeKind::Outside,
                );
            }
        }
        if let Some(p) = ungroup.dnd_release_payload::<DragAccount>() {
            actions.push(Action::MoveToGroup(p.0.clone(), None));
        }
        ui.add_space(4.0);

        let footer = 58.0;
        let list_height = (ui.available_height() - footer).max(60.0);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for id in self.cfg.accounts_in(None) {
                    if let Some(a) = self.cfg.account(&id) {
                        self.account_row(ui, a, 0.0, actions);
                    }
                }
                for g in &self.cfg.groups {
                    ui.add_space(4.0);
                    self.group_header(ui, g, actions);
                    if !g.collapsed {
                        for id in self.cfg.accounts_in(Some(&g.id)) {
                            if let Some(a) = self.cfg.account(&id) {
                                self.account_row(ui, a, GROUP_INDENT, actions);
                            }
                        }
                    }
                }
                if self.cfg.accounts.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(tr!(
                            "Nenhuma conta ainda. Clique em \"Nova conta\" para começar.",
                            "No accounts yet. Click \"New account\" to start."
                        ))
                        .color(weak(dark)),
                    );
                }
            });

        ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("⚙").frame(false))
                    .side_tip(tr!("Configurações", "Settings"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::Settings);
                }
                ui.label(
                    RichText::new(format!("v{}", update::CURRENT_VERSION))
                        .small()
                        .color(weak(dark)),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    // the app's process tree includes every account's browser
                    let total = self.metrics.get(metrics::TOTAL).unwrap_or_default();
                    let (mut cpu, mut mem) = (0.0f32, 0u64);
                    for id in self.panels.keys() {
                        if let Some(u) = self.metrics.get(id) {
                            cpu += u.cpu;
                            mem += u.memory;
                        }
                    }
                    let n = self.panels.len();
                    let text = format!(
                        "CPU {:.0}% · RAM {}",
                        total.cpu,
                        metrics::fmt_memory(total.memory)
                    );
                    let rest = if self.economy {
                        tr!(
                            "Interface e navegador compartilhado",
                            "Interface and shared browser"
                        )
                    } else {
                        tr!("Interface", "Interface")
                    };
                    let detail = trf!(
                        "Total do Idlerx (todas as contas + interface)\n\
                         Contas abertas ({n}): CPU {cpu:.0}% · RAM {}\n\
                         {rest}: CPU {:.0}% · RAM {}\n\n\
                         CPU: parte do processador do PC inteiro (100% = todos os núcleos).\n\
                         RAM: memória em uso pelos processos (conjunto de trabalho).\n\
                         GPU (placa de vídeo) não é medida.",
                        "Idlerx total (all accounts + interface)\n\
                         Open accounts ({n}): CPU {cpu:.0}% · RAM {}\n\
                         {rest}: CPU {:.0}% · RAM {}\n\n\
                         CPU: share of the whole PC's processor (100% = all cores).\n\
                         RAM: memory in use by the processes (working set).\n\
                         GPU (graphics card) is not measured.",
                        metrics::fmt_memory(mem),
                        (total.cpu - cpu).max(0.0),
                        metrics::fmt_memory(total.memory.saturating_sub(mem))
                    );
                    ui.label(RichText::new(text).small().color(weak(dark)))
                        .side_tip(detail);
                    if self.economy {
                        leaf(ui).side_tip(tr!(
                            "Modo econômico ativado\n\n\
                             Um navegador compartilhado por todas as contas, cada uma \
                             no seu perfil. Pode ser desligado em Configurações.",
                            "Economy mode on\n\n\
                             One browser shared by all accounts, each in its own \
                             profile. It can be turned off in Settings."
                        ));
                    }
                });
            });
            ui.separator();
        });
    }

    pub(super) fn status_text(&self, id: &str) -> (String, Color32, bool) {
        if let Some(p) = self.panels.get(id) {
            let mut s = metrics::fmt_duration(p.started.elapsed());
            if let Some(u) = self.metrics.get(id) {
                s.push_str(" · ");
                s.push_str(&usage_text(u));
            }
            (s, ONLINE, true)
        } else if self.queue.iter().any(|q| q == id) {
            (tr!("abrindo…", "opening…").into(), ACCENT, true)
        } else if self.failed.contains_key(id) {
            (tr!("erro ao abrir", "failed to open").into(), DANGER, false)
        } else {
            (tr!("pausada", "paused").into(), Color32::GRAY, false)
        }
    }

    pub(super) fn account_row(
        &self,
        ui: &mut egui::Ui,
        a: &Account,
        indent: f32,
        actions: &mut Vec<Action>,
    ) {
        let dark = ui.visuals().dark_mode;
        let selected = self.cfg.selected.as_deref() == Some(a.id.as_str());
        let (full, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ROW_HEIGHT),
            Sense::click_and_drag(),
        );
        let rect = Rect::from_min_max(full.min + Vec2::new(indent, 0.0), full.max);
        resp.dnd_set_drag_payload(DragAccount(a.id.clone()));
        let dragging_me = resp.dragged();
        let drop_here = resp
            .dnd_hover_payload::<DragAccount>()
            .is_some_and(|p| p.0 != a.id);
        // dragged from below: the account goes after this one, so the line
        // is drawn at the bottom
        let from_below = resp.dnd_hover_payload::<DragAccount>().is_some_and(|p| {
            let order = self.cfg.display_order();
            let pos = |id: &str| order.iter().position(|o| o == id);
            pos(&p.0) < pos(&a.id)
        });
        if let Some(p) = resp.dnd_release_payload::<DragAccount>() {
            actions.push(Action::Reorder(p.0.clone(), a.id.clone()));
        }

        let painter = ui.painter_at(rect);
        let bg = if selected {
            ACCENT.gamma_multiply(if dark { 0.22 } else { 0.16 })
        } else if resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(8), bg);
        if dragging_me {
            painter.rect_stroke(
                rect.shrink(1.0),
                CornerRadius::same(8),
                Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        }
        if drop_here {
            let y = if from_below {
                rect.bottom() - 1.0
            } else {
                rect.top() + 1.0
            };
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.0, y),
                    Pos2::new(rect.right() - 4.0, y),
                ],
                Stroke::new(2.5, ACCENT),
            );
        }

        let (status, dot, open) = self.status_text(&a.id);
        let center = Pos2::new(rect.left() + 24.0, rect.center().y);
        avatar(&painter, center, 17.0, a, open);
        if open {
            painter.circle(
                center + Vec2::new(12.0, 12.0),
                5.0,
                dot,
                Stroke::new(2.0, panel_bg(dark)),
            );
        }

        let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
        let text_left = rect.left() + 50.0;
        // the name leaves room for the star/sound icons, the second line for
        // the start/pause button (only while hovering)
        let name_w = rect.right() - text_left - 44.0;
        let second_w = rect.right() - text_left - if hovered { 34.0 } else { 6.0 };
        single_line(
            &painter,
            Pos2::new(text_left, rect.top() + 8.0),
            &a.name,
            FontId::proportional(14.5),
            ui.visuals().strong_text_color(),
            name_w,
        );
        // open: usage first (it matters more than the address)
        let second = if open {
            status
        } else {
            format!("{} · {status}", config::domain(&a.url))
        };
        single_line(
            &painter,
            Pos2::new(text_left, rect.top() + 29.0),
            &second,
            FontId::proportional(11.0),
            weak(dark),
            second_w,
        );

        // icons on the right: favorite and sound
        let mut x = rect.right() - 14.0;
        if a.favorite {
            painter.text(
                Pos2::new(x, rect.top() + 16.0),
                Align2::CENTER_CENTER,
                "★",
                FontId::proportional(14.0),
                FAVORITE,
            );
            x -= 18.0;
        }
        if a.muted {
            painter.text(
                Pos2::new(x, rect.top() + 16.0),
                Align2::CENTER_CENTER,
                "🔇",
                FontId::proportional(12.0),
                weak(dark),
            );
        } else if self.audio.contains(&a.id) {
            painter.text(
                Pos2::new(x, rect.top() + 16.0),
                Align2::CENTER_CENTER,
                "🔊",
                FontId::proportional(12.0),
                ACCENT,
            );
        }

        // quick start/pause button while hovering
        let mut button_clicked = false;
        if hovered && !dragging_me {
            let r = Rect::from_center_size(
                Pos2::new(rect.right() - 18.0, rect.bottom() - 16.0),
                Vec2::splat(22.0),
            );
            let (icon, hint) = if open {
                (
                    "⏸",
                    tr!(
                        "Pausar (fecha a página e libera memória)",
                        "Pause (closes the page and frees memory)"
                    ),
                )
            } else {
                ("▶", tr!("Abrir", "Open"))
            };
            let b = ui.place(r, egui::Button::new(icon).small()).side_tip(hint);
            if b.clicked() {
                button_clicked = true;
                actions.push(if open {
                    Action::Pause(a.id.clone())
                } else {
                    Action::Open(a.id.clone())
                });
            }
        }

        if !button_clicked {
            if resp.double_clicked() && !open {
                actions.push(Action::Open(a.id.clone()));
            } else if resp.clicked() {
                actions.push(Action::Select(a.id.clone()));
            }
        }

        let resp = resp.side_tip(&a.url);
        resp.context_menu(|ui| self.account_menu(ui, a, open, actions));
    }

    pub(super) fn account_menu(
        &self,
        ui: &mut egui::Ui,
        a: &Account,
        open: bool,
        actions: &mut Vec<Action>,
    ) {
        let id = || a.id.clone();
        if open {
            if ui.button(tr!("⏸  Pausar", "⏸  Pause")).clicked() {
                actions.push(Action::Pause(id()));
            }
            if ui.button(tr!("⟳  Recarregar", "⟳  Reload")).clicked() {
                actions.push(Action::Reload(id()));
            }
        } else if ui.button(tr!("▶  Abrir", "▶  Open")).clicked() {
            actions.push(Action::Open(id()));
        }
        if ui
            .button(tr!("⛶  Mostrar sozinha", "⛶  Show alone"))
            .clicked()
        {
            actions.push(Action::Open(id()));
            actions.push(Action::Expand(id()));
        }
        ui.separator();
        if ui.button(tr!("✏  Editar…", "✏  Edit…")).clicked() {
            actions.push(Action::Edit(id()));
        }
        let fav = if a.favorite {
            tr!("☆  Desfavoritar", "☆  Unfavorite")
        } else {
            tr!("★  Favoritar", "★  Favorite")
        };
        if ui.button(fav).clicked() {
            actions.push(Action::ToggleFavorite(id()));
        }
        let mute = if a.muted {
            tr!("🔊  Ativar som", "🔊  Unmute")
        } else {
            tr!("🔇  Silenciar", "🔇  Mute")
        };
        if ui.button(mute).clicked() {
            actions.push(Action::ToggleMute(id()));
        }
        ui.menu_button(
            tr!("      Mover para o grupo", "      Move to group"),
            |ui| {
                let current = self.cfg.group_of(a);
                if ui
                    .selectable_label(current.is_none(), tr!("Sem grupo", "No group"))
                    .clicked()
                {
                    actions.push(Action::MoveToGroup(id(), None));
                }
                for g in &self.cfg.groups {
                    if ui
                        .selectable_label(current == Some(g.id.as_str()), &g.name)
                        .clicked()
                    {
                        actions.push(Action::MoveToGroup(id(), Some(g.id.clone())));
                    }
                }
                ui.separator();
                if ui.button(tr!("+ Novo grupo…", "+ New group…")).clicked() {
                    actions.push(Action::NewGroup(Some(id())));
                }
            },
        );
        ui.menu_button(
            tr!(
                "      Quadros por segundo (FPS)",
                "      Frames per second (FPS)"
            ),
            |ui| {
                let n = |v: u32| {
                    if v == 0 {
                        tr!("sem limite", "no limit").to_string()
                    } else {
                        v.to_string()
                    }
                };
                let auto = trf!(
                    "Automático ({} selecionada, {} nas outras)",
                    "Automatic ({} when selected, {} otherwise)",
                    n(self.cfg.fps_selected),
                    n(self.cfg.fps_limit)
                );
                if ui.selectable_label(a.fps.is_none(), auto).clicked() {
                    actions.push(Action::SetFps(id(), None));
                }
                if ui
                    .selectable_label(a.fps == Some(0), tr!("Sempre sem limite", "Never limited"))
                    .clicked()
                {
                    actions.push(Action::SetFps(id(), Some(0)));
                }
                for n in [60u32, 30, 15, 10, 5] {
                    if ui
                        .selectable_label(a.fps == Some(n), trf!("Sempre {n}", "Always {n}"))
                        .clicked()
                    {
                        actions.push(Action::SetFps(id(), Some(n)));
                    }
                }
            },
        );
        let grid = if a.in_grid {
            tr!("☐  Tirar da grade", "☐  Hide from grid")
        } else {
            tr!("☑  Mostrar na grade", "☑  Show in grid")
        };
        if ui.button(grid).clicked() {
            actions.push(Action::ToggleGrid(id()));
        }
        ui.separator();
        if ui
            .button(tr!(
                "⟲  Limpar dados (sair da conta)…",
                "⟲  Clear data (log out)…"
            ))
            .clicked()
        {
            actions.push(Action::AskClear(id()));
        }
        if ui
            .button(RichText::new(tr!("🗑  Remover…", "🗑  Remove…")).color(DANGER))
            .clicked()
        {
            actions.push(Action::AskRemove(id()));
        }
    }

    pub(super) fn group_header(&self, ui: &mut egui::Ui, g: &Group, actions: &mut Vec<Action>) {
        let dark = ui.visuals().dark_mode;
        // a group with a connected VPN shows its IP and traffic below the name
        let vpn_line = self.vpn_line(g);
        let height = GROUP_HEIGHT + if vpn_line.is_some() { 12.0 } else { 0.0 };
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height),
            Sense::click_and_drag(),
        );
        let ids = self.cfg.accounts_in(Some(&g.id));
        let open = ids.iter().filter(|id| self.is_open_or_opening(id)).count();
        // the grid is showing only this group
        let showing = self.cfg.layout == Layout::Grid
            && self.cfg.grid_group.as_deref() == Some(g.id.as_str());

        // an account dropped here joins the group
        let drop_here = resp.dnd_hover_payload::<DragAccount>().is_some();
        if let Some(p) = resp.dnd_release_payload::<DragAccount>() {
            actions.push(Action::MoveToGroup(p.0.clone(), Some(g.id.clone())));
        }
        // another group dropped here takes this place
        resp.dnd_set_drag_payload(DragGroup(g.id.clone()));
        let group_over = resp
            .dnd_hover_payload::<DragGroup>()
            .filter(|p| p.0 != g.id)
            .map(|p| {
                let pos = |id: &str| self.cfg.groups.iter().position(|x| x.id == id);
                pos(&p.0) < pos(&g.id)
            });
        if let Some(p) = resp.dnd_release_payload::<DragGroup>() {
            actions.push(Action::ReorderGroup(p.0.clone(), g.id.clone()));
        }

        let painter = ui.painter_at(rect);
        if let Some(from_above) = group_over {
            let y = if from_above {
                rect.bottom() - 1.0
            } else {
                rect.top() + 1.0
            };
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 4.0, y),
                    Pos2::new(rect.right() - 4.0, y),
                ],
                Stroke::new(2.5, ACCENT),
            );
        }
        if showing {
            painter.rect_filled(
                rect,
                CornerRadius::same(6),
                ACCENT.gamma_multiply(if dark { 0.22 } else { 0.16 }),
            );
        }
        if drop_here {
            painter.rect_filled(rect, CornerRadius::same(6), ACCENT.gamma_multiply(0.25));
            painter.rect_stroke(
                rect.shrink(1.0),
                CornerRadius::same(6),
                Stroke::new(1.5, ACCENT),
                StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(6),
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }

        // expand/collapse triangle
        let c = Pos2::new(rect.left() + 12.0, rect.top() + GROUP_HEIGHT / 2.0);
        let tri = if g.collapsed {
            vec![
                c + Vec2::new(-3.0, -5.0),
                c + Vec2::new(4.0, 0.0),
                c + Vec2::new(-3.0, 5.0),
            ]
        } else {
            vec![
                c + Vec2::new(-5.0, -3.0),
                c + Vec2::new(5.0, -3.0),
                c + Vec2::new(0.0, 4.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(tri, weak(dark), Stroke::NONE));

        let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
        let count = format!("{open}/{}", ids.len());
        let mid = rect.top() + GROUP_HEIGHT / 2.0;
        let mut right = rect.right() - if hovered { 58.0 } else { 8.0 };
        let count_rect = painter.text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            &count,
            FontId::proportional(11.0),
            weak(dark),
        );
        right = count_rect.left() - 4.0;
        // the group's VPN: a shield in its state color (details in the tooltip)
        let badge = self.vpn_badge(g);
        if let Some((color, _)) = &badge {
            let r = painter.text(
                Pos2::new(right, mid),
                Align2::RIGHT_CENTER,
                "🛡",
                FontId::proportional(11.0),
                *color,
            );
            right = r.left() - 4.0;
        }
        single_line(
            &painter,
            Pos2::new(rect.left() + 24.0, rect.top() + 7.0),
            &g.name,
            FontId::proportional(13.0),
            ui.visuals().strong_text_color(),
            right - 6.0 - (rect.left() + 24.0),
        );
        if let Some(line) = &vpn_line {
            single_line(
                &painter,
                Pos2::new(rect.left() + 24.0, rect.top() + GROUP_HEIGHT - 6.0),
                line,
                FontId::proportional(10.5),
                weak(dark),
                rect.width() - 32.0,
            );
        }

        let mut button_clicked = false;
        if hovered {
            let mut x = rect.right() - 4.0;
            let mut button = |icon: &str, hint: &str| -> bool {
                let r = Rect::from_min_size(
                    Pos2::new(x - 24.0, rect.top() + 4.0),
                    Vec2::new(24.0, GROUP_HEIGHT - 8.0),
                );
                x -= 26.0;
                ui.place(r, egui::Button::new(icon).small())
                    .side_tip(hint)
                    .clicked()
            };
            if button("⏸", tr!("Pausar todas do grupo", "Pause all in the group")) {
                button_clicked = true;
                actions.push(Action::PauseGroup(g.id.clone()));
            }
            if button("▶", tr!("Abrir todas do grupo", "Open all in the group")) {
                button_clicked = true;
                actions.push(Action::OpenGroup(g.id.clone()));
            }
        }
        // the arrow collapses/expands; the rest shows the group in the grid
        // (a second click shows every account again)
        let on_arrow = resp
            .interact_pointer_pos()
            .is_some_and(|p| p.x < rect.left() + 24.0);
        if resp.clicked() && !button_clicked {
            if on_arrow {
                actions.push(Action::ToggleCollapse(g.id.clone()));
            } else {
                actions.push(Action::GridGroup(if showing {
                    None
                } else {
                    Some(g.id.clone())
                }));
            }
        }

        let (mut cpu, mut mem) = (0.0f32, 0u64);
        for id in &ids {
            if let Some(u) = self.metrics.get(id) {
                cpu += u.cpu;
                mem += u.memory;
            }
        }
        let vpn_tip = badge.map(|(_, t)| format!("{t}\n\n")).unwrap_or_default();
        let tip = trf!(
            "{vpn_tip}{} conta(s) · {open} aberta(s)\nCPU {cpu:.0}% · RAM {}\n\nClique para mostrar só este grupo na grade \
             (de novo: todas). A seta recolhe/expande. Arraste para mudar a ordem; \
             solte contas aqui para colocá-las no grupo.",
            "{vpn_tip}{} account(s) · {open} open\nCPU {cpu:.0}% · RAM {}\n\nClick to show only this group in the grid \
             (again: all). The arrow collapses/expands. Drag to reorder; drop \
             accounts here to put them in the group.",
            ids.len(),
            metrics::fmt_memory(mem)
        );
        let resp = resp.side_tip(tip);
        resp.context_menu(|ui| {
            if ui
                .button(tr!(
                    "+  Nova conta neste grupo",
                    "+  New account in this group"
                ))
                .clicked()
            {
                actions.push(Action::NewAccount(Some(g.id.clone())));
            }
            ui.separator();
            if ui.button(tr!("▶  Abrir todas", "▶  Open all")).clicked() {
                actions.push(Action::OpenGroup(g.id.clone()));
            }
            if ui.button(tr!("⏸  Pausar todas", "⏸  Pause all")).clicked() {
                actions.push(Action::PauseGroup(g.id.clone()));
            }
            if ui
                .button(tr!(
                    "⊞  Mostrar só este grupo na grade",
                    "⊞  Show only this group in the grid"
                ))
                .clicked()
            {
                actions.push(Action::GridGroup(Some(g.id.clone())));
            }
            ui.separator();
            let pos = self
                .cfg
                .groups
                .iter()
                .position(|x| x.id == g.id)
                .unwrap_or(0);
            if ui
                .add_enabled(
                    pos > 0,
                    egui::Button::new(tr!("      Mover para cima", "      Move up")),
                )
                .clicked()
            {
                actions.push(Action::MoveGroup(g.id.clone(), -1));
            }
            if ui
                .add_enabled(
                    pos + 1 < self.cfg.groups.len(),
                    egui::Button::new(tr!("      Mover para baixo", "      Move down")),
                )
                .clicked()
            {
                actions.push(Action::MoveGroup(g.id.clone(), 1));
            }
            ui.separator();
            if ui
                .button(tr!("✏  Editar (nome, VPN)…", "✏  Edit (name, VPN)…"))
                .clicked()
            {
                actions.push(Action::RenameGroup(g.id.clone()));
            }
            if g.active_vpn().is_some()
                && ui
                    .button(tr!("⟳  Reconectar a VPN", "⟳  Reconnect the VPN"))
                    .clicked()
            {
                actions.push(Action::RetryVpn(g.id.clone()));
            }
            if ui
                .button(
                    RichText::new(tr!(
                        "🗑  Excluir grupo (as contas ficam)",
                        "🗑  Delete group (accounts stay)"
                    ))
                    .color(DANGER),
                )
                .clicked()
            {
                actions.push(Action::DeleteGroup(g.id.clone()));
            }
        });
    }

    /// The account being dragged follows the pointer.
    pub(super) fn drag_ghost(&self, ctx: &egui::Context) {
        if let (Some(p), Some(pos)) = (
            egui::DragAndDrop::payload::<DragGroup>(ctx),
            ctx.pointer_latest_pos(),
        ) {
            if let Some(g) = self.cfg.group(&p.0) {
                let dark = Self::dark(ctx);
                egui::Area::new(egui::Id::new("drag_ghost"))
                    .order(egui::Order::Tooltip)
                    .fixed_pos(pos + Vec2::new(14.0, 10.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(panel_bg(dark))
                            .stroke(Stroke::new(1.0, ACCENT))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(8, 5))
                            .show(ui, |ui| {
                                ui.label(RichText::new(&g.name).strong());
                            });
                    });
            }
            return;
        }
        let Some(p) = egui::DragAndDrop::payload::<DragAccount>(ctx) else {
            return;
        };
        let (Some(a), Some(pos)) = (self.cfg.account(&p.0), ctx.pointer_latest_pos()) else {
            return;
        };
        let dark = Self::dark(ctx);
        egui::Area::new(egui::Id::new("drag_ghost"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos + Vec2::new(14.0, 10.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(panel_bg(dark))
                    .stroke(Stroke::new(1.0, ACCENT))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                            avatar(ui.painter(), r.center(), 10.0, a, true);
                            ui.label(RichText::new(&a.name).strong());
                        });
                    });
            });
    }
}
