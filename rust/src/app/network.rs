//! VPN of groups in the interface: each group's tunnel follows its open
//! accounts, accounts of a group with a VPN open only once it is up, the
//! group dialog's VPN section and the status shown in the list.

use super::*;
use crate::config::{GroupVpn, VpnAuth};
use crate::vpn::{self, Phase};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// OpenVPN Community download page (builds without the embedded installer).
const OPENVPN_DOWNLOADS: &str = "https://openvpn.net/community-downloads/";

/// VPN part of the group dialog.
#[derive(Default)]
pub(super) struct VpnEdit {
    pub enabled: bool,
    /// .ovpn in use (already copied), or the one just chosen.
    pub file: String,
    pub label: String,
    /// Chosen in this dialog: copied on save.
    pub chosen: Option<PathBuf>,
    pub auth: VpnAuth,
    pub username: String,
    pub password: String,
    pub seed: String,
    pub show_secrets: bool,
}

impl VpnEdit {
    pub fn of(group: Option<&Group>) -> VpnEdit {
        let Some(g) = group else {
            return VpnEdit::default();
        };
        let Some(v) = &g.vpn else {
            return VpnEdit::default();
        };
        let secret = vpn::secrets::get(&g.id);
        VpnEdit {
            enabled: v.enabled,
            file: v.file.clone(),
            label: v.label.clone(),
            chosen: None,
            auth: v.auth,
            username: v.username.clone(),
            password: secret.password,
            seed: secret.seed,
            show_secrets: false,
        }
    }
}

/// Result of the OpenVPN installation started from a placeholder.
pub(super) type Install = Arc<Mutex<Option<Result<bool, String>>>>;

impl App {
    /// Active VPN of an account's group: (group id, settings).
    pub(super) fn vpn_of(&self, id: &str) -> Option<(String, GroupVpn)> {
        let a = self.cfg.account(id)?;
        let g = self.cfg.group(self.cfg.group_of(a)?)?;
        g.active_vpn().map(|v| (g.id.clone(), v.clone()))
    }

    /// The account may open now: no VPN, or its group's VPN is up.
    pub(super) fn network_ready(&self, id: &str) -> bool {
        match self.vpn_of(id) {
            None => true,
            Some((g, _)) => self.tunnels.status(&g).up(),
        }
    }

    /// Browser arguments of an account: its group's gate when it has a VPN.
    pub(super) fn vpn_gate(&mut self, id: &str) -> Option<(String, u16)> {
        let (g, _) = self.vpn_of(id)?;
        let port = self.tunnels.port(&g)?;
        Some((g, port))
    }

    /// Closes and opens again an account whose network changed (VPN turned
    /// on or off, moved to or from a group with a VPN): its browser has to
    /// start over with other arguments.
    pub(super) fn restart_account(&mut self, id: &str) {
        if !self.is_open_or_opening(id) {
            return;
        }
        self.close_panel(id);
        self.request_open(id);
        // the old browser takes a moment to let its folder go
        self.retry_open.insert(
            id.to_string(),
            (0, Instant::now() + Duration::from_millis(1500)),
        );
    }

    /// Every frame: groups with a VPN and open accounts stay connected, the
    /// others are disconnected; pages are reloaded when a dropped tunnel
    /// comes back.
    pub(super) fn sync_vpn(&mut self) {
        let mut wanted = Vec::new();
        for g in &self.cfg.groups {
            if let Some(v) = g.active_vpn() {
                let ids = self.cfg.accounts_in(Some(&g.id));
                if ids.iter().any(|id| self.is_open_or_opening(id)) {
                    wanted.push((g.id.clone(), v.clone()));
                }
            }
        }
        for (gid, v) in &wanted {
            self.tunnels.ensure(gid, v);
        }
        for gid in self.tunnels.groups() {
            if !wanted.iter().any(|(g, _)| *g == gid) && self.tunnels.running(&gid) {
                self.tunnels.stop(&gid);
            }
        }
        for (gid, _) in &wanted {
            let epoch = self.tunnels.status(gid).epoch;
            let last = self.vpn_epochs.insert(gid.clone(), epoch).unwrap_or(epoch);
            if epoch > last && last > 0 {
                for id in self.cfg.accounts_in(Some(gid)) {
                    if let Some(p) = self.panels.get(&id) {
                        p.reload();
                    }
                }
            }
        }
        // the OpenVPN installation finished
        let done = self
            .installing
            .as_ref()
            .and_then(|i| i.lock().ok().and_then(|mut r| r.take()));
        if let Some(result) = done {
            self.installing = None;
            if result.is_ok() {
                for (gid, v) in &wanted {
                    if self.tunnels.status(gid).phase == Phase::MissingOpenVpn {
                        self.tunnels.retry(gid, v);
                    }
                }
            }
            self.install_result = Some(result);
        }
    }

    pub(super) fn install_openvpn(&mut self) {
        if self.installing.is_some() {
            return;
        }
        let slot: Install = Arc::default();
        let s = slot.clone();
        std::thread::spawn(move || {
            let r = vpn::installer::install();
            if let Ok(mut g) = s.lock() {
                *g = Some(r);
            }
        });
        self.installing = Some(slot);
        self.install_result = None;
    }

    /// Placeholder of an account waiting for its group's VPN (spinner,
    /// failure, OpenVPN missing). False when it is not waiting for one.
    pub(super) fn vpn_wait(&self, ui: &mut egui::Ui, id: &str, actions: &mut Vec<Action>) -> bool {
        let Some((gid, _)) = self.vpn_of(id) else {
            return false;
        };
        let status = self.tunnels.status(&gid);
        if status.up() {
            return false;
        }
        let dark = ui.visuals().dark_mode;
        let group = self
            .cfg
            .group(&gid)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        match &status.phase {
            Phase::Failed(msg) => {
                ui.label(
                    RichText::new(trf!(
                        "A VPN do grupo \"{group}\" não conectou:",
                        "The VPN of the \"{group}\" group did not connect:"
                    ))
                    .color(DANGER),
                );
                ui.label(RichText::new(msg).small());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(tr!("Tentar de novo", "Try again")).clicked() {
                        actions.push(Action::RetryVpn(gid.clone()));
                    }
                    if ui.button(tr!("Editar grupo…", "Edit group…")).clicked() {
                        actions.push(Action::RenameGroup(gid.clone()));
                    }
                });
            }
            Phase::MissingOpenVpn if self.installing.is_some() => {
                ui.spinner();
                ui.label(tr!(
                    "Instalando o OpenVPN… aprove o pedido do Windows.",
                    "Installing OpenVPN… approve the Windows prompt."
                ));
            }
            Phase::MissingOpenVpn => {
                ui.label(tr!(
                    "O grupo usa VPN, mas o OpenVPN Community não está instalado.",
                    "The group uses a VPN, but OpenVPN Community is not installed."
                ));
                if let Some(Err(e)) = &self.install_result {
                    ui.label(RichText::new(e).small().color(DANGER));
                }
                ui.add_space(4.0);
                if vpn::installer::embedded() {
                    let b = egui::Button::new(
                        RichText::new(tr!("Instalar o OpenVPN", "Install OpenVPN"))
                            .color(Color32::WHITE),
                    )
                    .fill(ACCENT);
                    if ui
                        .add(b)
                        .on_hover_text(trf!(
                            "Instalador oficial (OpenVPN {}), embutido no Idlerx. O Windows \
                             pede permissão de administrador.",
                            "Official installer (OpenVPN {}), embedded in Idlerx. Windows asks \
                             for administrator permission.",
                            vpn::installer::MSI_VERSION
                        ))
                        .clicked()
                    {
                        actions.push(Action::InstallOpenVpn);
                    }
                } else if ui
                    .button(tr!("Baixar o OpenVPN", "Download OpenVPN"))
                    .clicked()
                {
                    let _ = open::that(OPENVPN_DOWNLOADS);
                }
            }
            _ => {
                ui.spinner();
                ui.label(RichText::new(trf!(
                    "Conectando a VPN do grupo \"{group}\"…",
                    "Connecting the VPN of the \"{group}\" group…"
                )));
                if status.slow {
                    ui.label(
                        RichText::new(tr!(
                            "O servidor da VPN não está respondendo; continuo tentando.",
                            "The VPN server is not responding; still trying."
                        ))
                        .small()
                        .color(weak(dark)),
                    );
                }
                if status.phase == Phase::Reconnecting {
                    if let Some(e) = &status.last_error {
                        ui.label(RichText::new(e).small().color(weak(dark)));
                    }
                }
            }
        }
        true
    }

    /// Lock shown on a group with a VPN: color and tooltip.
    pub(super) fn vpn_badge(&self, g: &Group) -> Option<(Color32, String)> {
        let v = g.active_vpn()?;
        let s = self.tunnels.status(&g.id);
        let (color, state) = match &s.phase {
            Phase::Connected => (ONLINE, tr!("conectada", "connected").to_string()),
            Phase::Connecting => (FAVORITE, tr!("conectando…", "connecting…").to_string()),
            Phase::Reconnecting => (
                FAVORITE,
                tr!(
                    "caiu, reconectando… (contas sem internet até voltar)",
                    "dropped, reconnecting… (accounts offline until it is back)"
                )
                .to_string(),
            ),
            Phase::Failed(m) => (DANGER, trf!("não conectou: {m}", "did not connect: {m}")),
            Phase::MissingOpenVpn => (
                DANGER,
                tr!("OpenVPN não instalado", "OpenVPN not installed").to_string(),
            ),
            Phase::Stopped => (
                Color32::GRAY,
                tr!("desligada (nenhuma conta aberta)", "off (no open account)").to_string(),
            ),
        };
        let mut tip = trf!("VPN: {state}", "VPN: {state}");
        if s.up() {
            if let Some(ip) = s.ip {
                tip.push_str(&trf!(
                    "\nIP para os sites: {ip}",
                    "\nIP seen by the sites: {ip}"
                ));
            }
            tip.push_str(&format!(
                "\n⏷ {}  ⏶ {}",
                vpn::fmt_rate(s.down_rate),
                vpn::fmt_rate(s.up_rate)
            ));
        }
        if s.down_total + s.up_total > 0 {
            tip.push_str(&trf!(
                "\nTotal: ⏷ {}  ⏶ {}",
                "\nTotal: ⏷ {}  ⏶ {}",
                vpn::fmt_bytes(s.down_total),
                vpn::fmt_bytes(s.up_total)
            ));
        }
        tip.push_str(&trf!("\nArquivo: {}", "\nFile: {}", v.label));
        tip.push_str(tr!(
            "\n\nAs contas do grupo só acessam a internet pela VPN: se ela cair, ficam \
             sem conexão até ela voltar.",
            "\n\nThe group's accounts reach the internet only through the VPN: if it \
             drops, they stay offline until it comes back."
        ));
        Some((color, tip))
    }

    /// Short line under the group name: IP and traffic while connected.
    pub(super) fn vpn_line(&self, g: &Group) -> Option<String> {
        g.active_vpn()?;
        let s = self.tunnels.status(&g.id);
        if !s.up() {
            return None;
        }
        let ip = s.ip.map(|ip| ip.to_string()).unwrap_or_default();
        Some(format!(
            "{ip}  ⏷ {}  ⏶ {}",
            vpn::fmt_rate(s.down_rate),
            vpn::fmt_rate(s.up_rate)
        ))
    }

    /// Saves the VPN part of the group dialog. Err = message for the user.
    pub(super) fn save_group_vpn(&mut self, gid: &str, st: &mut VpnEdit) -> Result<(), String> {
        let had = self.cfg.group(gid).and_then(|g| g.vpn.clone());
        if !st.enabled && st.file.is_empty() && st.chosen.is_none() {
            // never set up: nothing to keep
            if let Some(g) = self.cfg.group_mut(gid) {
                g.vpn = None;
            }
            return Ok(());
        }
        if st.enabled {
            if st.file.is_empty() && st.chosen.is_none() {
                return Err(tr!(
                    "Escolha o arquivo .ovpn da VPN.",
                    "Choose the VPN .ovpn file."
                )
                .into());
            }
            if st.auth.uses_user() && st.username.trim().is_empty() {
                return Err(tr!("Informe o usuário da VPN.", "Enter the VPN username.").into());
            }
            if st.auth.uses_password() && st.password.is_empty() {
                return Err(tr!("Informe a senha da VPN.", "Enter the VPN password.").into());
            }
            if st.auth.uses_token() && vpn::totp::normalize_seed(&st.seed).is_none() {
                return Err(tr!(
                    "Seed do token inválida: copie a chave (base32) do cadastro do Google \
                     Authenticator.",
                    "Invalid token seed: copy the key (base32) from the Google Authenticator \
                     enrollment."
                )
                .into());
            }
        }
        if let Some(src) = st.chosen.take() {
            let text = std::fs::read_to_string(&src).map_err(|e| {
                trf!(
                    "Não consegui ler o arquivo: {e}",
                    "Could not read the file: {e}"
                )
            })?;
            st.label = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            st.file = if vpn::self_contained(&text) {
                // a copy of its own: the original can be moved or deleted
                let dest = vpn::dir().join(format!("{gid}.ovpn"));
                std::fs::create_dir_all(vpn::dir())
                    .and_then(|_| std::fs::write(&dest, &text))
                    .map_err(|e| {
                        trf!(
                            "Não consegui copiar o arquivo: {e}",
                            "Could not copy the file: {e}"
                        )
                    })?;
                dest.to_string_lossy().into_owned()
            } else {
                // it uses certificate files next to it: kept where it is
                src.to_string_lossy().into_owned()
            };
        }
        let seed = vpn::totp::normalize_seed(&st.seed).unwrap_or_default();
        let secret = vpn::secrets::Secret {
            password: if st.auth.uses_password() {
                st.password.clone()
            } else {
                String::new()
            },
            seed: if st.auth.uses_token() {
                seed
            } else {
                String::new()
            },
        };
        if secret != vpn::secrets::get(gid) {
            vpn::secrets::set(gid, secret).map_err(|e| {
                trf!(
                    "Não consegui guardar a senha: {e}",
                    "Could not store the password: {e}"
                )
            })?;
            self.tunnels.secrets_changed();
        }
        let new = GroupVpn {
            enabled: st.enabled,
            file: st.file.clone(),
            label: st.label.clone(),
            auth: st.auth,
            username: st.username.trim().to_string(),
        };
        let was_active = had
            .as_ref()
            .is_some_and(|v| v.enabled && !v.file.is_empty());
        if let Some(g) = self.cfg.group_mut(gid) {
            g.vpn = Some(new.clone());
        }
        // the open accounts change network: through the VPN or not anymore
        if was_active != new.enabled || had.as_ref().map(|v| &v.file) != Some(&new.file) {
            for id in self.cfg.accounts_in(Some(gid)) {
                self.restart_account(&id);
            }
        }
        Ok(())
    }

    /// Forgets a group's VPN files (group deleted).
    pub(super) fn forget_group_vpn(&mut self, gid: &str) {
        self.tunnels.stop(gid);
        vpn::secrets::remove(gid);
        let _ = std::fs::remove_file(vpn::dir().join(format!("{gid}.ovpn")));
    }
}

/// VPN section of the group dialog.
pub(super) fn vpn_section(ui: &mut egui::Ui, st: &mut VpnEdit, economy: bool) {
    let dark = ui.visuals().dark_mode;
    ui.checkbox(
        &mut st.enabled,
        tr!(
            "Usar VPN (OpenVPN) neste grupo",
            "Use a VPN (OpenVPN) in this group"
        ),
    )
    .on_hover_text(tr!(
        "As contas do grupo só acessam a internet pela VPN, nunca pela sua conexão: \
         se a VPN cair, elas ficam sem internet até ela voltar. O resto do computador \
         e os outros grupos não passam por ela.",
        "The group's accounts reach the internet only through the VPN, never through \
         your connection: if the VPN drops, they stay offline until it is back. The \
         rest of the computer and the other groups do not go through it."
    ));
    if !st.enabled {
        return;
    }
    if economy {
        ui.label(
            RichText::new(tr!(
                "No modo econômico, as contas do grupo usam um navegador só delas: \
                 ao ligar ou desligar a VPN, entre nos jogos de novo uma vez.",
                "In economy mode, the group's accounts use a browser of their own: \
                 after turning the VPN on or off, log in to the games once again."
            ))
            .small()
            .color(weak(dark)),
        );
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let name = match (&st.chosen, st.label.is_empty()) {
            (Some(p), _) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            (None, false) => st.label.clone(),
            (None, true) => tr!("Nenhum arquivo escolhido", "No file chosen").into(),
        };
        if ui.button(tr!("Escolher .ovpn…", "Choose .ovpn…")).clicked() {
            let picked = rfd::FileDialog::new()
                .set_title(tr!("Arquivo da VPN (OpenVPN)", "VPN file (OpenVPN)"))
                .add_filter("OpenVPN", &["ovpn", "conf"])
                .pick_file();
            if let Some(p) = picked {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                // the file says whether a username and password are needed
                if vpn::asks_password(&text) {
                    if !st.auth.uses_user() {
                        st.auth = VpnAuth::Password;
                    }
                } else {
                    st.auth = VpnAuth::Certificate;
                }
                st.chosen = Some(p);
            }
        }
        ui.label(RichText::new(name).color(weak(dark)));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(tr!("Login:", "Login:"));
        let label = |a: VpnAuth| match a {
            VpnAuth::Certificate => tr!("Só certificado", "Certificate only"),
            VpnAuth::Password => tr!("Usuário e senha", "Username and password"),
            VpnAuth::Token => tr!("Usuário e token", "Username and token"),
            VpnAuth::PasswordAndToken => {
                tr!("Usuário, senha e token", "Username, password and token")
            }
        };
        egui::ComboBox::from_id_salt("vpn_auth")
            .selected_text(label(st.auth))
            .width(200.0)
            .show_ui(ui, |ui| {
                for a in [
                    VpnAuth::Certificate,
                    VpnAuth::Password,
                    VpnAuth::Token,
                    VpnAuth::PasswordAndToken,
                ] {
                    ui.selectable_value(&mut st.auth, a, label(a));
                }
            });
    });
    if st.auth.uses_user() {
        egui::Grid::new("vpn_login")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr!("Usuário", "Username"));
                ui.add(egui::TextEdit::singleline(&mut st.username).desired_width(220.0));
                ui.end_row();
                if st.auth.uses_password() {
                    ui.label(tr!("Senha", "Password"));
                    ui.add(
                        egui::TextEdit::singleline(&mut st.password)
                            .password(!st.show_secrets)
                            .desired_width(220.0),
                    );
                    ui.end_row();
                }
                if st.auth.uses_token() {
                    ui.label(tr!("Seed do token", "Token seed"));
                    ui.add(
                        egui::TextEdit::singleline(&mut st.seed)
                            .password(!st.show_secrets)
                            .hint_text("JBSWY3DPEHPK3PXP…")
                            .desired_width(220.0),
                    )
                    .on_hover_text(tr!(
                        "A chave (base32) do cadastro do Google Authenticator: o Idlerx gera \
                         o código de 6 dígitos a cada conexão.",
                        "The key (base32) from the Google Authenticator enrollment: Idlerx \
                         generates the 6-digit code on each connection."
                    ));
                    ui.end_row();
                }
            });
        if st.auth.uses_password() || st.auth.uses_token() {
            ui.checkbox(&mut st.show_secrets, tr!("Mostrar", "Show"));
        }
        ui.label(
            RichText::new(tr!(
                "Guardados neste computador, criptografados pelo Windows.",
                "Kept on this computer, encrypted by Windows."
            ))
            .small()
            .color(weak(dark)),
        );
    }
}
