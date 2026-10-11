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
                let modal = egui::Modal::new(egui::Id::new("edit")).show(ctx, |ui| {
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
                        ui.checkbox(&mut st.open_now, tr!("Abrir agora", "Open now"));
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
                let modal = egui::Modal::new(egui::Id::new("group")).show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui.heading(if st.id.is_some() {
                        tr!("Renomear grupo", "Rename group")
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
                    match &st.id {
                        Some(gid) => {
                            if let Some(g) = self.cfg.group_mut(gid) {
                                g.name = name;
                            }
                        }
                        None => {
                            let gid = config::new_id();
                            self.cfg.groups.push(Group {
                                id: gid.clone(),
                                name,
                                collapsed: false,
                            });
                            if let Some(acc) = &st.assign {
                                self.cfg.move_to_group(acc, Some(&gid));
                            }
                        }
                    }
                    self.mark_dirty();
                    keep = false;
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
                let a = Account::new(name, url);
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
                    "Impede o navegador de desacelerar páginas escondidas ou atrás de outras. \
                     Vale para as contas abertas depois da mudança.",
                    "Stops the browser from slowing down hidden or covered pages. \
                     Applies to accounts opened after the change."
                ))
                .small()
                .color(weak(dark)),
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tr!(
                    "Quadros por segundo das contas não selecionadas",
                    "Frames per second of the accounts not selected"
                ));
                let label = |n: u32| {
                    if n == 0 {
                        tr!("Sem limite", "No limit").to_string()
                    } else {
                        format!("{n} FPS")
                    }
                };
                egui::ComboBox::from_id_salt("fps_limit")
                    .selected_text(label(self.cfg.fps_limit))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for n in [0u32, 60, 30, 15, 10, 5] {
                            ui.selectable_value(&mut self.cfg.fps_limit, n, label(n));
                        }
                    });
            });
            ui.label(
                RichText::new(tr!(
                    "Jogos idle não precisam desenhar 60 vezes por segundo: limitar economiza bastante \
                     CPU e GPU na grade. A conta selecionada desenha sem limite. Cada conta pode ter \
                     o seu (botão direito › Quadros por segundo).",
                    "Idle games do not need to draw 60 times a second: a limit saves a lot of CPU and \
                     GPU in the grid. The selected account draws freely. Each account can have its \
                     own (right click › Frames per second)."
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
