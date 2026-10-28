//! Dialogs: account, group, settings, update and confirmations.

use super::*;

impl App {
    // ------------------------------------------------------------ dialogs --

    pub(super) fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let dialog = match dialog {
            Dialog::Edit(mut st) => {
                let groups: Vec<(String, String)> = self
                    .cfg
                    .groups
                    .iter()
                    .map(|g| (g.id.clone(), g.name.clone()))
                    .collect();
                let modal = egui::Modal::new(egui::Id::new("edit")).show(ctx, |ui| {
                    let dark = ui.visuals().dark_mode;
                    ui.set_width(420.0);
                    let title = if st.id.is_some() {
                        tr!("Editar conta", "Edit account")
                    } else {
                        tr!("Nova conta", "New account")
                    };
                    ui.heading(title);
                    ui.add_space(8.0);
                    ui.label(tr!("Endereço do jogo", "Game address"));
                    let url = ui.add(
                        egui::TextEdit::singleline(&mut st.url)
                            .hint_text("https://…")
                            .desired_width(f32::INFINITY),
                    );
                    if st.id.is_none() && st.url.is_empty() && st.name.is_empty() {
                        url.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.label(tr!(
                        "Nome (para você reconhecer a conta)",
                        "Name (so you can tell the accounts apart)"
                    ));
                    let hint = config::normalize_url(&st.url)
                        .map(|u| config::name_from_url(&u))
                        .unwrap_or_else(|| tr!("Ex.: Char principal", "E.g. Main char").into());
                    ui.add(
                        egui::TextEdit::singleline(&mut st.name)
                            .hint_text(hint)
                            .desired_width(f32::INFINITY),
                    );
                    if st.id.is_none() {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut st.open_now, tr!("Abrir agora", "Open now"));
                            // the group it goes into (discreet: only when there are groups)
                            if !groups.is_empty() {
                                ui.add_space(12.0);
                                ui.label(RichText::new(tr!("Grupo:", "Group:")).color(weak(dark)));
                                let none = tr!("Sem grupo", "No group").to_string();
                                let current = st
                                    .group
                                    .as_ref()
                                    .and_then(|g| groups.iter().find(|(id, _)| id == g))
                                    .map(|(_, n)| n.clone())
                                    .unwrap_or_else(|| none.clone());
                                egui::ComboBox::from_id_salt("new_account_group")
                                    .selected_text(current)
                                    .width(160.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut st.group, None, none);
                                        for (id, name) in &groups {
                                            ui.selectable_value(
                                                &mut st.group,
                                                Some(id.clone()),
                                                name,
                                            );
                                        }
                                    });
                            }
                        });
                    }
                    if let Some(e) = &st.error {
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    ui.add_space(10.0);
                    let mut save = ui.input(|i| i.key_pressed(Key::Enter));
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(tr!("Salvar", "Save")).color(Color32::WHITE),
                                )
                                .fill(ACCENT),
                            )
                            .clicked()
                        {
                            save = true;
                        }
                        if ui.button(tr!("Cancelar", "Cancel")).clicked() {
                            cancel = true;
                        }
                    });
                    (save, cancel)
                });
                let (save, cancel) = modal.inner;
                if cancel || modal.should_close() {
                    keep = false;
                } else if save {
                    match config::normalize_url(&st.url) {
                        None => {
                            st.error = Some(
                                tr!(
                                    "Endereço inválido. Ex.: https://www.jogo.com",
                                    "Invalid address. E.g. https://www.game.com"
                                )
                                .into(),
                            )
                        }
                        Some(url) => {
                            let name = match st.name.trim() {
                                "" => config::name_from_url(&url),
                                n => n.to_string(),
                            };
                            self.save_account(&st, name, url, actions);
                            keep = false;
                        }
                    }
                }
                Dialog::Edit(st)
            }
            Dialog::Group(mut st) => {
                let economy = self.cfg.economy;
                let modal = egui::Modal::new(egui::Id::new("group")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.heading(if st.id.is_some() {
                        tr!("Editar grupo", "Edit group")
                    } else {
                        tr!("Novo grupo", "New group")
                    });
                    ui.add_space(8.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut st.name)
                            .hint_text(tr!(
                                "Ex.: Tibia, Contas principais",
                                "E.g. Tibia, Main accounts"
                            ))
                            .desired_width(f32::INFINITY),
                    );
                    if !r.has_focus() && st.name.is_empty() {
                        r.request_focus();
                    }
                    ui.add_space(8.0);
                    // a color that marks the group and its accounts
                    ui.horizontal(|ui| {
                        ui.label(tr!("Cor:", "Color:"));
                        let none = ui
                            .selectable_label(st.color.is_none(), tr!("nenhuma", "none"));
                        if none.clicked() {
                            st.color = None;
                        }
                        for i in 0..GROUP_COLORS.len() as u8 {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 7.0, group_color(i));
                            if st.color == Some(i) {
                                ui.painter().circle_stroke(
                                    r.center(),
                                    9.0,
                                    Stroke::new(1.5, ui.visuals().strong_text_color()),
                                );
                            }
                            if resp.clicked() {
                                st.color = Some(i);
                            }
                        }
                    });
                    ui.add_space(10.0);
                    ui.checkbox(
                        &mut st.in_private,
                        tr!(
                            "Modo InPrivate: as contas não guardam nada e abrem numa página em branco",
                            "InPrivate mode: the accounts keep nothing and open on a blank page"
                        ),
                    )
                    .on_hover_text(tr!(
                        "Cookies, login, senhas e dados dos sites ficam só na memória e somem \
                         ao pausar a conta ou fechar o Idlerx. Jogos que salvam o progresso no \
                         navegador perdem esse progresso.",
                        "Cookies, logins, passwords and site data stay in memory only and are \
                         gone when the account is paused or Idlerx closes. Games that save \
                         progress in the browser lose it."
                    ));
                    ui.add_space(6.0);
                    ui.separator();
                    network::vpn_section(ui, &mut st.vpn, economy);
                    if let Some(e) = &st.error {
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    ui.add_space(10.0);
                    let mut save = ui.input(|i| i.key_pressed(Key::Enter));
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(tr!("Salvar", "Save")).color(Color32::WHITE),
                                )
                                .fill(ACCENT),
                            )
                            .clicked()
                        {
                            save = true;
                        }
                        if ui.button(tr!("Cancelar", "Cancel")).clicked() {
                            cancel = true;
                        }
                    });
                    (save, cancel)
                });
                let (save, cancel) = modal.inner;
                let name = st.name.trim().to_string();
                if cancel || modal.should_close() {
                    keep = false;
                } else if save && !name.is_empty() {
                    let gid = match &st.id {
                        Some(gid) => {
                            if let Some(g) = self.cfg.group_mut(gid) {
                                g.name = name;
                            }
                            gid.clone()
                        }
                        None => {
                            let gid = config::new_id();
                            self.cfg.groups.push(Group {
                                id: gid.clone(),
                                name,
                                ..Group::default()
                            });
                            gid
                        }
                    };
                    if let Some(g) = self.cfg.group_mut(&gid) {
                        g.color = st.color;
                    }
                    // InPrivate changed: the group's open accounts start over
                    let was_private = self.cfg.group(&gid).is_some_and(|g| g.in_private);
                    if was_private != st.in_private {
                        if let Some(g) = self.cfg.group_mut(&gid) {
                            g.in_private = st.in_private;
                        }
                        for id in self.cfg.accounts_in(Some(&gid)) {
                            if let Some(a) = self.cfg.account_mut(&id) {
                                a.last_url = None;
                            }
                            self.restart_account(&id);
                        }
                    }
                    match self.save_group_vpn(&gid, &mut st.vpn) {
                        Ok(()) => {
                            if let Some(acc) = st.assign.take() {
                                let before = self.browser_key(&acc);
                                self.cfg.move_to_group(&acc, Some(&gid));
                                if self.browser_key(&acc) != before {
                                    self.restart_account(&acc);
                                }
                            }
                            keep = false;
                        }
                        Err(e) => {
                            // a new group stays (with its name): the dialog
                            // goes on editing it
                            st.id = Some(gid);
                            st.error = Some(e);
                        }
                    }
                    self.mark_dirty();
                }
                Dialog::Group(st)
            }
            Dialog::Settings => {
                if self.settings_dialog(ctx) {
                    keep = false;
                }
                Dialog::Settings
            }
            Dialog::Update => {
                if update_dialog(ctx) {
                    keep = false;
                    update::clear_result();
                }
                Dialog::Update
            }
            Dialog::ConfirmRemove(id) => {
                let name = self
                    .cfg
                    .account(&id)
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                match confirm(
                    ctx,
                    &trf!("Remover \"{name}\"?", "Remove \"{name}\"?"),
                    tr!(
                        "A conta sai da lista e os dados dela neste computador (login, cookies, cache) são apagados.",
                        "The account leaves the list and its data on this computer (login, cookies, cache) is deleted."
                    ),
                    tr!("Remover", "Remove"),
                ) {
                    Some(true) => {
                        self.remove_account(&id);
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None => {}
                }
                Dialog::ConfirmRemove(id)
            }
            Dialog::ConfirmClear(id) => {
                let name = self
                    .cfg
                    .account(&id)
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                match confirm(
                    ctx,
                    &trf!("Limpar os dados de \"{name}\"?", "Clear the data of \"{name}\"?"),
                    tr!(
                        "Login, senhas salvas, cookies e cache desta conta serão apagados: ela volta como nova e você precisará entrar no jogo de novo.",
                        "Login, saved passwords, cookies and cache of this account will be deleted: it starts fresh and you will need to log in to the game again."
                    ),
                    tr!("Limpar dados", "Clear data"),
                ) {
                    Some(true) => {
                        self.clear_account(&id);
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None => {}
                }
                Dialog::ConfirmClear(id)
            }
            Dialog::Macro(mut st) => {
                if !self.macro_dialog(ctx, &mut st) {
                    keep = false;
                }
                Dialog::Macro(st)
            }
            Dialog::ConfirmDeleteGroup(gid, mut with_accounts) => {
                let name = self
                    .cfg
                    .group(&gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                let ids = self.cfg.accounts_in(Some(&gid));
                let modal = egui::Modal::new(egui::Id::new("delete_group")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.heading(trf!(
                        "Excluir o grupo \"{name}\"?",
                        "Delete the \"{name}\" group?"
                    ));
                    ui.add_space(6.0);
                    if ids.is_empty() {
                        ui.label(tr!("O grupo está vazio.", "The group is empty."));
                    } else {
                        ui.checkbox(
                            &mut with_accounts,
                            trf!(
                                "Excluir também as {} conta(s) deste grupo",
                                "Also delete the {} account(s) in this group",
                                ids.len()
                            ),
                        );
                        let text = if with_accounts {
                            RichText::new(tr!(
                                "As contas saem da lista e os dados delas neste computador \
                                 (login, cookies, cache) são apagados.",
                                "The accounts leave the list and their data on this computer \
                                 (login, cookies, cache) is deleted."
                            ))
                            .color(DANGER)
                        } else {
                            RichText::new(tr!(
                                "As contas ficam na lista, sem grupo.",
                                "The accounts stay in the list, without a group."
                            ))
                        };
                        ui.label(text);
                    }
                    ui.add_space(10.0);
                    let mut r = None;
                    ui.horizontal(|ui| {
                        let yes = if with_accounts {
                            tr!("Excluir grupo e contas", "Delete group and accounts")
                        } else {
                            tr!("Excluir grupo", "Delete group")
                        };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(yes).color(Color32::WHITE))
                                    .fill(DANGER),
                            )
                            .clicked()
                        {
                            r = Some(true);
                        }
                        if ui.button(tr!("Cancelar", "Cancel")).clicked() {
                            r = Some(false);
                        }
                    });
                    r
                });
                match modal.inner {
                    Some(true) => {
                        if with_accounts {
                            for id in &ids {
                                self.remove_account(id);
                            }
                        }
                        actions.push(Action::DeleteGroup(gid.clone()));
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None if modal.should_close() => keep = false,
                    None => {}
                }
                Dialog::ConfirmDeleteGroup(gid, with_accounts)
            }
            Dialog::ConfirmQuit => {
                match confirm(
                    ctx,
                    tr!("Fechar o Idlerx?", "Close Idlerx?"),
                    tr!(
                        "Todas as contas abertas serão fechadas e os jogos param de rodar aqui até você abrir o Idlerx de novo.",
                        "All open accounts will be closed and the games stop running here until you open Idlerx again."
                    ),
                    tr!("Fechar", "Close"),
                ) {
                    Some(true) => {
                        self.quit(ctx);
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None => {}
                }
                Dialog::ConfirmQuit
            }
        };
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    pub(super) fn save_account(
        &mut self,
        st: &EditState,
        name: String,
        url: String,
        actions: &mut Vec<Action>,
    ) {
        match &st.id {
            Some(id) => {
                let reload = {
                    let Some(a) = self.cfg.account_mut(id) else {
                        return;
                    };
                    a.name = name;
                    let changed = a.url != url;
                    if changed {
                        a.url = url.clone();
                        a.last_url = None;
                    }
                    changed
                };
                if reload {
                    if let Some(p) = self.panels.get(id) {
                        p.load(&url);
                    }
                }
            }
            None => {
                let mut a = Account::new(name, url);
                a.group = st.group.clone().filter(|g| self.cfg.group(g).is_some());
                // a collapsed group opens to show its new account
                if let Some(g) = a.group.clone().and_then(|g| self.cfg.group_mut(&g)) {
                    g.collapsed = false;
                }
                let id = a.id.clone();
                self.cfg.accounts.push(a);
                if st.open_now {
                    actions.push(Action::Open(id));
                } else {
                    actions.push(Action::Select(id));
                }
            }
        }
        self.mark_dirty();
    }

    /// Returns true when it should close.
    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) -> bool {
        let before = self.cfg.clone();
        let mut quit = false;
        let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(tr!("Configurações", "Settings"));
            ui.add_space(10.0);
            let dark = ui.visuals().dark_mode;

            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    ui.label(tr!("Idioma", "Language"));
                    let current = match self.cfg.language.as_deref() {
                        Some("pt") => "Português",
                        Some("en") => "English",
                        _ => tr!("Automático (Windows)", "Automatic (Windows)"),
                    };
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(current)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.cfg.language,
                                None,
                                tr!("Automático (Windows)", "Automatic (Windows)"),
                            );
                            ui.selectable_value(
                                &mut self.cfg.language,
                                Some("pt".into()),
                                "Português",
                            );
                            ui.selectable_value(
                                &mut self.cfg.language,
                                Some("en".into()),
                                "English",
                            );
                        });
                    ui.end_row();

                    ui.label(tr!("Tema", "Theme"));
                    let current = match self.cfg.theme {
                        Theme::System => tr!("Do sistema", "System"),
                        Theme::Dark => tr!("Escuro", "Dark"),
                        Theme::Light => tr!("Claro", "Light"),
                    };
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(current)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.cfg.theme,
                                Theme::System,
                                tr!("Do sistema", "System"),
                            );
                            ui.selectable_value(
                                &mut self.cfg.theme,
                                Theme::Dark,
                                tr!("Escuro", "Dark"),
                            );
                            ui.selectable_value(
                                &mut self.cfg.theme,
                                Theme::Light,
                                tr!("Claro", "Light"),
                            );
                        });
                    ui.end_row();
                });

            ui.add_space(10.0);
            ui.checkbox(
                &mut self.cfg.restore_open,
                tr!(
                    "Reabrir as contas que estavam abertas",
                    "Reopen the accounts that were open"
                ),
            );
            ui.checkbox(
                &mut self.cfg.keep_active,
                tr!(
                    "Manter os jogos ativos em segundo plano",
                    "Keep games running in the background"
                ),
            );
            ui.label(
                RichText::new(tr!(
                    "Impede o navegador de desacelerar páginas escondidas ou atrás de outras, \
                     e o jogo de uma conta fora da tela continua rodando (a 5 quadros por \
                     segundo). Vale para as contas abertas depois da mudança.",
                    "Stops the browser from slowing down hidden or covered pages, and the \
                     game of an account off screen keeps running (at 5 frames per second). \
                     Applies to accounts opened after the change."
                ))
                .small()
                .color(weak(dark)),
            );
            ui.checkbox(
                &mut self.cfg.quiet_muted,
                tr!(
                    "Contas sem som não processam o áudio",
                    "Muted accounts don't process sound"
                ),
            );
            ui.label(
                RichText::new(tr!(
                    "Com a conta muda, o som do jogo fica parado em vez de seguir tocando sem                      sair no alto-falante (cerca de 1% de um núcleo de CPU a menos por conta).                      Desligue se algum jogo mudar de comportamento quando está mudo.",
                    "While an account is muted, the game's sound is stopped instead of playing                      on with nothing coming out (about 1% of a CPU core less per account). Turn                      it off if a game behaves differently while muted."
                ))
                .small()
                .color(weak(dark)),
            );

            ui.add_space(6.0);
            let label = |n: u32| {
                if n == 0 {
                    tr!("Sem limite", "No limit").to_string()
                } else {
                    format!("{n} FPS")
                }
            };
            egui::Grid::new("fps_grid")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr!(
                        "Quadros por segundo: conta selecionada",
                        "Frames per second: selected account"
                    ));
                    egui::ComboBox::from_id_salt("fps_selected")
                        .selected_text(label(self.cfg.fps_selected))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for n in [0u32, 120, 60, 30, 15] {
                                ui.selectable_value(&mut self.cfg.fps_selected, n, label(n));
                            }
                        });
                    ui.end_row();
                    ui.label(tr!(
                        "Quadros por segundo: outras contas",
                        "Frames per second: other accounts"
                    ));
                    egui::ComboBox::from_id_salt("fps_limit")
                        .selected_text(label(self.cfg.fps_limit))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for n in [0u32, 60, 30, 15, 10, 5] {
                                ui.selectable_value(&mut self.cfg.fps_limit, n, label(n));
                            }
                        });
                    ui.end_row();
                });
            ui.label(
                RichText::new(tr!(
                    "Jogos idle não precisam desenhar 60 vezes por segundo (nem 144 ou 180, a \
                     frequência do monitor): limitar economiza bastante CPU e GPU. Cada conta pode \
                     ter o seu (botão direito › Quadros por segundo); o quadro da grade mostra \
                     quantos ela realmente desenha.",
                    "Idle games do not need to draw 60 times a second (nor 144 or 180, the monitor's \
                     rate): a limit saves a lot of CPU and GPU. Each account can have its own \
                     (right click › Frames per second); the grid cell shows how many it really \
                     draws."
                ))
                .small()
                .color(weak(dark)),
            );

            ui.add_space(6.0);
            if ui
                .checkbox(
                    &mut self.cfg.save_passwords,
                    tr!("Salvar senhas das contas", "Save the accounts' passwords"),
                )
                .changed()
            {
                for p in self.panels.values() {
                    p.set_save_passwords(self.cfg.save_passwords);
                }
            }
            ui.label(
                RichText::new(tr!(
                    "O navegador oferece salvar o login de cada conta e preenche sozinho na \
                     próxima vez. Cada conta tem as próprias senhas, guardadas neste computador \
                     e criptografadas pelo Windows (como no Chrome e no Edge); sem sincronização.",
                    "The browser offers to save each account's login and fills it in next time. \
                     Each account has its own passwords, kept on this computer and encrypted by \
                     Windows (like Chrome and Edge); no sync."
                ))
                .small()
                .color(weak(dark)),
            );

            ui.add_space(6.0);
            self.blocker_settings(ui);

            ui.add_space(6.0);
            ui.checkbox(
                &mut self.cfg.economy,
                tr!(
                    "Modo econômico: um navegador compartilhado por todas as contas (recomendado)",
                    "Economy mode: one browser shared by all accounts (recommended)"
                ),
            );
            ui.label(
                RichText::new(tr!(
                    "Gasta bem menos memória (cerca de 100 MB a menos por conta). As contas \
                     continuam isoladas (login, cookies e cache próprios), mas se o navegador \
                     compartilhado travar, todas caem juntas. Os logins de cada modo ficam \
                     guardados separados: ao trocar, entre nos jogos de novo uma vez.",
                    "Uses much less memory (about 100 MB less per account). Accounts stay \
                     isolated (their own login, cookies and cache), but if the shared browser \
                     crashes, all of them go down together. Each mode keeps its own logins: \
                     after switching, log in to the games once again."
                ))
                .small()
                .color(weak(dark)),
            );
            let needs_restart = self.cfg.economy != self.economy
                || (self.economy && self.cfg.keep_active != self.keep_active);
            if needs_restart {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(tr!(
                            "Vale ao reiniciar o Idlerx.",
                            "Applies after restarting Idlerx."
                        ))
                        .small()
                        .color(FAVORITE),
                    );
                    if ui.button(tr!("Reiniciar agora", "Restart now")).clicked() {
                        self.restart = true;
                    }
                });
            }
            ui.add_space(6.0);
            ui.checkbox(
                &mut self.cfg.close_to_tray,
                tr!(
                    "O X manda o Idlerx para a bandeja (os jogos continuam rodando)",
                    "The X sends Idlerx to the tray (the games keep running)"
                ),
            );
            ui.add_space(6.0);
            ui.checkbox(
                &mut self.cfg.check_updates,
                tr!(
                    "Verificar novas versões automaticamente",
                    "Check for new versions automatically"
                ),
            );
            ui.horizontal(|ui| {
                if ui.button(tr!("Verificar agora", "Check now")).clicked() {
                    update::check_now();
                }
                match update::state() {
                    update::State::Checking => {
                        ui.spinner();
                    }
                    update::State::UpToDate => {
                        ui.label(
                            RichText::new(tr!(
                                "✔ Você tem a versão mais recente.",
                                "✔ You have the latest version."
                            ))
                            .color(ONLINE),
                        );
                    }
                    update::State::CheckFailed(e) => {
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    _ => {
                        if let Some(n) = update::available() {
                            if ui
                                .link(trf!("Versão {n} disponível", "Version {n} available"))
                                .clicked()
                            {
                                self.dialog = Some(Dialog::Update);
                            }
                        }
                    }
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(tr!("Pasta dos dados", "Data folder")).clicked() {
                    let dir = config::data_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = open::that(dir);
                }
                if ui
                    .button(tr!("Perfis das contas", "Account profiles"))
                    .clicked()
                {
                    let dir = config::profiles_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = open::that(dir);
                }
                if ui
                    .button(tr!("Registro das páginas", "Pages log"))
                    .on_hover_text(tr!(
                        "Quando cada conta carregou, recarregou (e por quê), ficou escondida ou visível.",
                        "When each account loaded, reloaded (and why), was hidden or shown."
                    ))
                    .clicked()
                {
                    let file = config::data_dir().join("logs").join("pages.log");
                    if !file.exists() {
                        crate::events::log("Idlerx", "registro iniciado");
                    }
                    let _ = open::that(file);
                }
            });

            ui.add_space(12.0);
            ui.separator();
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Idlerx v{}", update::CURRENT_VERSION)).color(weak(dark)),
                );
                if ui
                    .link(tr!("código-fonte (GPL-3.0)", "source code (GPL-3.0)"))
                    .clicked()
                {
                    let _ = open::that(update::repository_page());
                }
                if let Ok(v) = &self.runtime {
                    ui.label(
                        RichText::new(format!("· WebView2 {v}"))
                            .small()
                            .color(weak(dark)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(tr!("Fechar", "Close")).clicked() {
                        close = true;
                    }
                    if ui.button(tr!("Sair do Idlerx", "Quit Idlerx")).clicked() {
                        quit = true;
                    }
                });
            });
            close
        });

        if self.cfg.block_ads != before.block_ads
            || self.cfg.block_allow != before.block_allow
            || self.cfg.block_rules != before.block_rules
        {
            crate::blocker::configure(
                self.cfg.block_ads,
                &self.cfg.block_allow,
                &self.cfg.block_rules,
            );
        }
        if self.cfg.language != before.language {
            i18n::apply(
                self.cfg
                    .language
                    .as_deref()
                    .and_then(i18n::Language::from_code),
            );
        }
        if self.cfg.theme != before.theme {
            apply_style(ctx, self.cfg.theme);
        }
        CHECK_UPDATES.store(self.cfg.check_updates, Ordering::Relaxed);
        if self.cfg != before {
            self.mark_dirty();
        }
        if quit {
            self.dialog = Some(Dialog::ConfirmQuit);
            return true;
        }
        modal.inner || modal.should_close()
    }
}

/// Returns true when it should close.
fn update_dialog(ctx: &egui::Context) -> bool {
    let modal = egui::Modal::new(egui::Id::new("update")).show(ctx, |ui| {
        ui.set_width(420.0);
        let mut close = false;
        match update::state() {
            update::State::Available(v) => {
                ui.heading(trf!("Versão {} disponível", "Version {} available", v.number));
                ui.label(trf!("Você está na versão {}.", "You are on version {}.", update::CURRENT_VERSION));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if v.installable()
                        && ui.add(egui::Button::new(RichText::new(tr!("Atualizar agora", "Update now")).color(Color32::WHITE)).fill(ACCENT)).clicked() {
                            update::install(v.clone());
                        }
                    if ui.button(tr!("Ver novidades", "What's new")).clicked() {
                        let _ = open::that(&v.page);
                    }
                    if ui.button(tr!("Depois", "Later")).clicked() {
                        close = true;
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(tr!(
                    "O Idlerx baixa a versão nova, confere o arquivo e reabre sozinho com as mesmas contas.",
                    "Idlerx downloads the new version, verifies the file and reopens by itself with the same accounts."
                ))
                .small());
            }
            update::State::Downloading(v, f) => {
                ui.heading(trf!("Baixando a versão {}…", "Downloading version {}…", v.number));
                ui.add(egui::ProgressBar::new(f).show_percentage());
            }
            update::State::InstallFailed(v, e) => {
                ui.heading(tr!("Não foi possível atualizar", "Could not update"));
                ui.label(RichText::new(e).color(DANGER));
                ui.horizontal(|ui| {
                    if ui.button(tr!("Tentar de novo", "Try again")).clicked() {
                        update::install(v.clone());
                    }
                    if ui.button(tr!("Baixar pela página", "Download from the page")).clicked() {
                        let _ = open::that(&v.page);
                    }
                    if ui.button(tr!("Fechar", "Close")).clicked() {
                        close = true;
                    }
                });
            }
            update::State::Restarting(_) => {
                ui.heading(tr!("Reiniciando…", "Restarting…"));
                ui.spinner();
            }
            _ => close = true,
        }
        close
    });
    modal.inner || modal.should_close()
}

/// Yes/no question. Some(true) = confirmed, Some(false) = cancelled.
fn confirm(ctx: &egui::Context, title: &str, text: &str, yes: &str) -> Option<bool> {
    let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.heading(title);
        ui.add_space(6.0);
        ui.label(text);
        ui.add_space(10.0);
        let mut r = None;
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(RichText::new(yes).color(Color32::WHITE)).fill(DANGER))
                .clicked()
            {
                r = Some(true);
            }
            if ui.button(tr!("Cancelar", "Cancel")).clicked() {
                r = Some(false);
            }
        });
        r
    });
    if modal.should_close() {
        return Some(false);
    }
    modal.inner
}

impl App {
    /// Ad blocker part of the settings.
    fn blocker_settings(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        ui.checkbox(
            &mut self.cfg.block_ads,
            tr!("Bloquear anúncios e rastreadores", "Block ads and trackers"),
        )
        .on_hover_text(tr!(
            "Com o motor do navegador Brave e as listas que o uBlock Origin usa: filtros do              uBlock, EasyList, EasyPrivacy e a lista de Peter Lowe. Vale para todas as contas,              inclusive InPrivate. Ligar ou desligar vale para as contas abertas depois.",
            "With the Brave browser's engine and the lists uBlock Origin uses: uBlock filters,              EasyList, EasyPrivacy and Peter Lowe's list. Applies to every account, InPrivate              included. Turning it on or off applies to accounts opened afterwards."
        ));
        if !self.cfg.block_ads {
            return;
        }
        let st = crate::blocker::status();
        ui.horizontal(|ui| {
            let mut line = if st.rules > 0 {
                trf!(
                    "{} regras · {} bloqueados nesta sessão",
                    "{} rules · {} blocked this session",
                    st.rules,
                    crate::blocker::blocked()
                )
            } else {
                tr!("Baixando as listas…", "Downloading the lists…").to_string()
            };
            if let Some(age) = st.updated.and_then(|t| t.elapsed().ok()) {
                line.push_str(&trf!(
                    " · listas de {}",
                    " · lists from {}",
                    metrics::fmt_duration(age)
                ));
                line.push_str(tr!(" atrás", " ago"));
            }
            ui.label(RichText::new(line).small().color(weak(dark)));
            if st.updating {
                ui.spinner();
            } else if ui
                .small_button(tr!("Atualizar listas", "Update lists"))
                .clicked()
            {
                crate::blocker::refresh_now();
            }
        });
        if let Some(e) = &st.error {
            ui.label(RichText::new(e).small().color(DANGER));
        }
        egui::CollapsingHeader::new(tr!("Sites liberados", "Allowed sites"))
            .id_salt("block_allow")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!(
                        "Um por linha (ex.: jogo.com.br): nada é bloqueado nas páginas deles.",
                        "One per line (e.g. game.com): nothing is blocked on their pages."
                    ))
                    .small()
                    .color(weak(dark)),
                );
                let mut text = self.cfg.block_allow.join("\n");
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.cfg.block_allow = text.split('\n').map(String::from).collect();
                }
            });
        egui::CollapsingHeader::new(tr!("Elementos escondidos", "Hidden elements"))
            .id_salt("block_rules")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!(
                        "Os que você escondeu com \"Bloquear elemento\" (botão 🛡 da conta), um por \
                         linha: site##seletor. Apague uma linha para o elemento voltar.",
                        "The ones you hid with \"Block element\" (an account's 🛡 button), one per \
                         line: site##selector. Delete a line to bring the element back."
                    ))
                    .small()
                    .color(weak(dark)),
                );
                let mut text = self.cfg.block_rules.join("\n");
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut text)
                            .code_editor()
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.cfg.block_rules = text
                        .split('\n')
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                }
            });
    }
}
