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
            Dialog::Passwords(id) => {
                if !self.passwords_dialog(ctx, &id) {
                    keep = false;
                }
                Dialog::Passwords(id)
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
            Dialog::FirefoxIntro => {
                let modal = egui::Modal::new(egui::Id::new("firefox_intro")).show(ctx, |ui| {
                    let dark = ui.visuals().dark_mode;
                    ui.set_width(460.0);
                    ui.heading(tr!("O Idlerx agora usa o Firefox", "Idlerx now uses Firefox"));
                    ui.add_space(6.0);
                    for line in [
                        tr!(
                            "• Cada conta é um contêiner do Firefox: login, cookies e dados \
                             próprios, como antes.",
                            "• Each account is a Firefox container: its own login, cookies and \
                             data, as before."
                        ),
                        tr!(
                            "• O uBlock Origin completo bloqueia anúncios, rastreadores e pop-ups \
                             em todas as contas. O escudo 🛡 no cabeçalho da conta abre o painel \
                             dele (liberar um site, remover um banner).",
                            "• The full uBlock Origin blocks ads, trackers and pop-ups in every \
                             account. The shield 🛡 in the account's header opens its panel \
                             (allow a site, remove a banner)."
                        ),
                        tr!(
                            "• Os logins, as senhas salvas e os saves que os jogos guardam no \
                             navegador vêm do Edge sozinhos, conta por conta. Se algum jogo \
                             pedir login, entre uma vez.",
                            "• Each account's logins, saved passwords and the saves games keep \
                             in the browser come over from Edge by themselves. If a game asks \
                             you to log in, do it once."
                        ),
                        tr!(
                            "• Ctrl+1…9 mostra a conta daquela posição da lista, também de dentro \
                             do jogo.",
                            "• Ctrl+1…9 shows the account at that place of the list, also from \
                             inside the game."
                        ),
                        tr!(
                            "• O Idlerx usa perfis próprios do Firefox: o seu Firefox de sempre \
                             não muda.",
                            "• Idlerx uses Firefox profiles of its own: your everyday Firefox \
                             does not change."
                        ),
                    ] {
                        ui.label(line);
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr!(
                            "Os dados antigos do Edge ficam guardados até você apagar \
                             (Configurações › Apagar dados do Edge).",
                            "The old Edge data stays until you delete it (Settings › Delete \
                             Edge data)."
                        ))
                        .small()
                        .color(weak(dark)),
                    );
                    ui.add_space(8.0);
                    ui.add(
                        egui::Button::new(RichText::new(tr!("Entendi", "Got it")).color(Color32::WHITE))
                            .fill(ACCENT),
                    )
                    .clicked()
                });
                if modal.inner || modal.should_close() {
                    keep = false;
                    self.cfg.firefox_intro_shown = true;
                    self.mark_dirty();
                }
                Dialog::FirefoxIntro
            }
            Dialog::ConfirmDeleteEdge => {
                match confirm(
                    ctx,
                    tr!("Apagar os dados do Edge?", "Delete the Edge data?"),
                    tr!(
                        "Apaga as pastas da versão anterior do Idlerx (motor do Edge). Os logins, \
                         senhas e saves que vieram para o Firefox continuam. Não dá para desfazer.",
                        "Deletes the folders of Idlerx's previous version (Edge engine). The \
                         logins, passwords and saves that came over to Firefox stay. It cannot \
                         be undone."
                    ),
                    tr!("Apagar", "Delete"),
                ) {
                    Some(true) => {
                        crate::migrate::delete_old();
                        events::log("Idlerx", "dados do Edge apagados");
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None => {}
                }
                Dialog::ConfirmDeleteEdge
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

            ui.add_space(6.0);
            let label = |n: u32| {
                if n == 0 {
                    tr!("Sem limite", "No limit").to_string()
                } else {
                    format!("{n} FPS")
                }
            };
            ui.horizontal(|ui| {
                ui.label(tr!("Quadros por segundo das páginas", "Frames per second of the pages"));
                egui::ComboBox::from_id_salt("fps_limit")
                    .selected_text(label(self.cfg.fps_limit))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for n in [0u32, 60, 30, 20, 15, 10] {
                            ui.selectable_value(&mut self.cfg.fps_limit, n, label(n));
                        }
                    });
            });
            ui.label(
                RichText::new(tr!(
                    "Jogos idle não precisam desenhar 60 vezes por segundo (nem 144 ou 180, a \
                     frequência do monitor): limitar economiza bastante CPU e GPU. O limite é \
                     feito pelo próprio Firefox, para todas as contas.",
                    "Idle games do not need to draw 60 times a second (nor 144 or 180, the monitor's \
                     rate): a limit saves a lot of CPU and GPU. The limit is applied by Firefox \
                     itself, to every account."
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
                let on = self.cfg.save_passwords;
                crate::firefox::with(|e| e.set_save_passwords(on));
            }
            ui.label(
                RichText::new(tr!(
                    "O Idlerx oferece salvar o login de cada conta e preenche sozinho na \
                     próxima vez. Cada conta tem as próprias senhas (uma conta nunca vê as da \
                     outra), guardadas neste computador e criptografadas pelo Windows; sem \
                     sincronização. Botão direito na conta › Senhas salvas.",
                    "Idlerx offers to save each account's login and fills it in next time. \
                     Each account has its own passwords (an account never sees another's), \
                     kept on this computer and encrypted by Windows; no sync. Right-click \
                     the account › Saved passwords."
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
                    "Modo econômico: poucos processos de página para todas as contas (recomendado)",
                    "Economy mode: a few page processes for all accounts (recommended)"
                ),
            );
            ui.label(
                RichText::new(tr!(
                    "Gasta bem menos memória: contas do mesmo jogo dividem processos. As contas \
                     continuam isoladas (login, cookies e dados próprios), mas se um desses \
                     processos travar, as contas nele recarregam juntas. Desligado: um processo \
                     por conta e site (mais memória, isolamento total).",
                    "Uses much less memory: accounts of the same game share processes. Accounts \
                     stay isolated (their own login, cookies and data), but if one of those \
                     processes crashes, the accounts in it reload together. Off: one process \
                     per account and site (more memory, full isolation)."
                ))
                .small()
                .color(weak(dark)),
            );
            let needs_restart = self.cfg.economy != self.economy
                || self.cfg.keep_active != self.keep_active
                || self.cfg.fps_limit != self.fps;
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
                if !crate::migrate::old_dirs().is_empty()
                    && ui
                        .button(tr!("Apagar dados do Edge…", "Delete Edge data…"))
                        .on_hover_text(tr!(
                            "Dados da versão anterior do Idlerx (motor do Edge). Os logins, \
                             senhas e saves das contas já vieram para o Firefox; apague quando \
                             tiver conferido que os seus jogos estão em dia.",
                            "Data of Idlerx's previous version (Edge engine). The accounts' \
                             logins, passwords and saves already came over to Firefox; delete \
                             it once you have checked that your games are fine."
                        ))
                        .clicked()
                {
                    self.dialog = Some(Dialog::ConfirmDeleteEdge);
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
                        RichText::new(format!("· Firefox {v}"))
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
    /// Ad blocker part of the settings: uBlock Origin, in every account.
    fn blocker_settings(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        ui.label(tr!(
            "Anúncios e rastreadores: uBlock Origin",
            "Ads and trackers: uBlock Origin"
        ));
        ui.label(
            RichText::new(tr!(
                "A extensão completa, em todas as contas (inclusive InPrivate), com as listas \
                 dela atualizadas sozinhas. Para liberar um site ou remover um banner teimoso: \
                 o escudo 🛡 no cabeçalho da conta.",
                "The full extension, in every account (InPrivate included), with its lists \
                 updated by themselves. To allow a site or remove a stubborn banner: the \
                 shield 🛡 in the account's header."
            ))
            .small()
            .color(weak(dark)),
        );
        if ui
            .small_button(tr!("Painel do uBlock Origin", "uBlock Origin dashboard"))
            .clicked()
        {
            crate::firefox::with(|e| e.open_blocker_dashboard());
        }
    }
}

impl App {
    /// Saved passwords of an account: which sites and user names, and
    /// deleting them (the passwords themselves are never shown).
    /// Returns false when it should close.
    fn passwords_dialog(&mut self, ctx: &egui::Context, id: &str) -> bool {
        let Some(a) = self.cfg.account(id).cloned() else {
            return false;
        };
        let list = crate::passwords::list(&a.profile);
        let modal = egui::Modal::new(egui::Id::new("passwords")).show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.set_width(440.0);
            ui.heading(trf!("Senhas salvas · {}", "Saved passwords · {}", a.name));
            ui.label(
                RichText::new(tr!(
                    "Só desta conta, guardadas neste computador e criptografadas pelo Windows. \
                     O Idlerx preenche sozinho quando a página de login abre.",
                    "This account's only, kept on this computer and encrypted by Windows. \
                     Idlerx fills them in when the login page opens."
                ))
                .small()
                .color(weak(dark)),
            );
            ui.add_space(8.0);
            if list.is_empty() {
                ui.label(
                    RichText::new(tr!("Nenhuma senha salva.", "No saved password."))
                        .color(weak(dark)),
                );
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for l in &list {
                    ui.horizontal(|ui| {
                        ui.label(config::host_of(&l.origin));
                        ui.label(RichText::new(&l.username).color(weak(dark)));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .small_button(tr!("Excluir", "Delete"))
                                .clicked()
                            {
                                crate::passwords::remove(&a.profile, &l.origin, &l.username);
                            }
                        });
                    });
                }
            });
            ui.add_space(8.0);
            ui.button(tr!("Fechar", "Close")).clicked()
        });
        !(modal.inner || modal.should_close())
    }
}
