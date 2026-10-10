//! User interface: account list on the left, navigation bar on top and the
//! open accounts (one at a time or in a grid) in the rest of the window.
//!
//! The pages are native child windows placed over the egui surface, so they
//! always cover anything egui paints in their area. Whenever egui needs that
//! area (a dialog or a menu is open) every page is hidden for the moment.

use crate::config::{self, Account, Config, Layout, Theme};
use crate::metrics::{self, Metrics};
use crate::panels::{self, Hotkey, Panel};
use crate::{i18n, update};
use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Color32, CornerRadius, FontId, Key, Margin, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, TextFormat, Vec2,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(0x14, 0xb8, 0xa6);
const FAVORITE: Color32 = Color32::from_rgb(0xf5, 0xb3, 0x01);
const ONLINE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
const DANGER: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
const SIDEBAR_WIDTH: f32 = 272.0;
const ROW_HEIGHT: f32 = 52.0;
const CELL_HEADER: f32 = 28.0;
const GRID_GAP: f32 = 4.0;
const POLL: Duration = Duration::from_secs(1);
const SAVE_DELAY: Duration = Duration::from_secs(2);

static CHECK_UPDATES: AtomicBool = AtomicBool::new(true);

fn check_updates_enabled() -> bool {
    CHECK_UPDATES.load(Ordering::Relaxed)
}

/// Payload of an account dragged from the list.
#[derive(Clone)]
struct DragAccount(String);

enum Action {
    Select(String),
    Open(String),
    Pause(String),
    ToggleFavorite(String),
    ToggleGrid(String),
    ToggleMute(String),
    Reload(String),
    Edit(String),
    AskClear(String),
    AskRemove(String),
    /// Moves the first account to the place of the second one (and shows it
    /// in the grid).
    MoveTo(String, String),
    /// Shows only this account (Tabs layout).
    Expand(String),
    NewAccount,
}

struct EditState {
    id: Option<String>,
    name: String,
    url: String,
    open_now: bool,
    error: Option<String>,
}

enum Dialog {
    Edit(EditState),
    Settings,
    Update,
    ConfirmRemove(String),
    ConfirmClear(String),
}

pub struct App {
    cfg: Config,
    panels: HashMap<String, Panel>,
    /// Accounts whose page could not be created, with the reason.
    failed: HashMap<String, String>,
    /// Accounts waiting for their page, in order.
    queue: VecDeque<String>,
    /// First account of the queue while its page is being created.
    starting: Option<(String, panels::PanelSlot)>,
    /// Main window (parent of the pages).
    hwnd: isize,
    metrics: Metrics,
    runtime: Result<String, String>,
    /// Address bar text and the account it belongs to.
    address: String,
    address_owner: Option<String>,
    address_error: bool,
    focus_mode: bool,
    dialog: Option<Dialog>,
    window_registered: bool,
    last_poll: Instant,
    audio: HashSet<String>,
    dirty: bool,
    changed_at: Instant,
    closing: bool,
    window_title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config) -> App {
        let ctx = cc.egui_ctx.clone();
        apply_style(&ctx, cfg.theme);

        let repaint = ctx.clone();
        let metrics = Metrics::start(move || repaint.request_repaint());

        let repaint = ctx.clone();
        update::ao_mudar(move || repaint.request_repaint());
        CHECK_UPDATES.store(cfg.check_updates, Ordering::Relaxed);
        update::iniciar_verificacao_periodica(check_updates_enabled);

        config::delete_pending(&mut cfg);
        let mut queue = VecDeque::new();
        for a in &mut cfg.accounts {
            if a.open && cfg.restore_open {
                queue.push_back(a.id.clone());
            } else {
                a.open = false;
            }
        }
        if cfg
            .selected
            .as_ref()
            .is_none_or(|s| cfg.account(s).is_none())
        {
            cfg.selected = cfg.display_order().into_iter().next();
        }

        App {
            cfg,
            panels: HashMap::new(),
            failed: HashMap::new(),
            queue,
            starting: None,
            hwnd: 0,
            metrics,
            runtime: panels::runtime_version(),
            address: String::new(),
            address_owner: None,
            address_error: false,
            focus_mode: false,
            dialog: None,
            window_registered: false,
            last_poll: Instant::now(),
            audio: HashSet::new(),
            dirty: false,
            changed_at: Instant::now(),
            closing: false,
            window_title: String::new(),
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.changed_at = Instant::now();
    }

    fn save(&mut self) {
        if config::save(&self.cfg).is_ok() {
            self.dirty = false;
        }
    }

    fn dark(ctx: &egui::Context) -> bool {
        ctx.global_style().visuals.dark_mode
    }

    // ---------------------------------------------------------- accounts --

    fn request_open(&mut self, id: &str) {
        let Some(a) = self.cfg.account_mut(id) else {
            return;
        };
        if !a.open {
            a.open = true;
            self.mark_dirty();
        }
        self.failed.remove(id);
        if !self.panels.contains_key(id) && !self.queue.iter().any(|q| q == id) {
            self.queue.push_back(id.to_string());
        }
    }

    fn close_panel(&mut self, id: &str) {
        self.panels.remove(id);
        self.queue.retain(|q| q != id);
        if self.starting.as_ref().is_some_and(|(s, _)| s == id) {
            self.starting = None;
        }
        self.failed.remove(id);
        self.audio.remove(id);
        if let Some(a) = self.cfg.account_mut(id) {
            a.open = false;
        }
        self.mark_dirty();
    }

    fn is_open_or_opening(&self, id: &str) -> bool {
        self.panels.contains_key(id) || self.queue.iter().any(|q| q == id)
    }

    /// Opens the accounts in the queue, one at a time. Creating a page is
    /// asynchronous: the window keeps responding while the browser starts.
    fn open_next(&mut self, ctx: &egui::Context) {
        if self.runtime.is_err() || self.hwnd == 0 {
            return;
        }
        if let Some((id, slot)) = &self.starting {
            let Some(result) = slot.borrow_mut().take() else {
                return; // still starting
            };
            let id = id.clone();
            self.starting = None;
            self.queue.retain(|q| q != &id);
            match result {
                Ok(p) if self.cfg.account(&id).is_some() => {
                    self.panels.insert(id, p);
                }
                Ok(_) => {} // removed meanwhile: the page is closed here
                Err(e) => {
                    self.failed.insert(id, e);
                }
            }
            ctx.request_repaint();
        }

        let Some(id) = self.queue.front().cloned() else {
            return;
        };
        match self.cfg.account(&id).cloned() {
            Some(acc) if !self.panels.contains_key(&id) => {
                let repaint = ctx.clone();
                let slot = panels::open(
                    self.hwnd,
                    &acc,
                    self.cfg.keep_active,
                    Self::dark(ctx),
                    move || repaint.request_repaint(),
                );
                self.starting = Some((id, slot));
            }
            _ => {
                self.queue.pop_front();
                ctx.request_repaint();
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Select(id) => {
                if self.cfg.selected.as_deref() != Some(&id) {
                    self.cfg.selected = Some(id);
                    self.mark_dirty();
                }
            }
            Action::Open(id) => {
                self.request_open(&id);
                self.cfg.selected = Some(id);
            }
            Action::Pause(id) => self.close_panel(&id),
            Action::ToggleFavorite(id) => {
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.favorite = !a.favorite;
                    self.mark_dirty();
                }
            }
            Action::ToggleGrid(id) => {
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.in_grid = !a.in_grid;
                    self.mark_dirty();
                }
            }
            Action::ToggleMute(id) => {
                let Some(a) = self.cfg.account_mut(&id) else {
                    return;
                };
                a.muted = !a.muted;
                let muted = a.muted;
                if let Some(p) = self.panels.get_mut(&id) {
                    p.set_muted(muted);
                }
                self.mark_dirty();
            }
            Action::Reload(id) => {
                if let Some(p) = self.panels.get(&id) {
                    p.reload();
                }
            }
            Action::Edit(id) => {
                if let Some(a) = self.cfg.account(&id) {
                    self.dialog = Some(Dialog::Edit(EditState {
                        id: Some(id.clone()),
                        name: a.name.clone(),
                        url: a.url.clone(),
                        open_now: false,
                        error: None,
                    }));
                }
            }
            Action::AskClear(id) => self.dialog = Some(Dialog::ConfirmClear(id)),
            Action::AskRemove(id) => self.dialog = Some(Dialog::ConfirmRemove(id)),
            Action::MoveTo(id, target) => {
                self.cfg.move_to(&id, &target);
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.in_grid = true;
                }
                self.request_open(&id);
                self.cfg.selected = Some(id);
                self.mark_dirty();
            }
            Action::Expand(id) => {
                self.cfg.layout = Layout::Tabs;
                self.cfg.selected = Some(id);
                self.mark_dirty();
            }
            Action::NewAccount => {
                self.dialog = Some(Dialog::Edit(EditState {
                    id: None,
                    name: String::new(),
                    url: String::new(),
                    open_now: true,
                    error: None,
                }))
            }
        }
    }

    fn remove_account(&mut self, id: &str) {
        self.close_panel(id);
        if let Some(pos) = self.cfg.accounts.iter().position(|a| a.id == id) {
            let a = self.cfg.accounts.remove(pos);
            self.cfg.pending_delete.push(a.profile.clone());
            config::delete_profile_later(a.profile);
        }
        if self.cfg.selected.as_deref() == Some(id) {
            self.cfg.selected = self.cfg.display_order().into_iter().next();
        }
        self.mark_dirty();
    }

    /// Logs the account out of everything: switches to a new, empty profile
    /// and deletes the old one.
    fn clear_account(&mut self, id: &str) {
        let was_open = self.is_open_or_opening(id);
        self.close_panel(id);
        if let Some(a) = self.cfg.account_mut(id) {
            let old = std::mem::replace(&mut a.profile, config::new_id());
            a.last_url = None;
            self.cfg.pending_delete.push(old.clone());
            config::delete_profile_later(old);
        }
        if was_open {
            self.request_open(id);
        }
        self.mark_dirty();
    }

    // ---------------------------------------------------------- per frame --

    /// Reads what changed inside the pages: address, zoom, sound, focus.
    fn poll(&mut self, ctx: &egui::Context) {
        let mut focused = None;
        let mut hotkeys = Vec::new();
        for (id, p) in &self.panels {
            if p.take_got_focus() {
                focused = Some(id.clone());
            }
            if let Some(k) = p.take_hotkey() {
                hotkeys.push(k);
            }
        }
        if let Some(id) = focused {
            self.apply(Action::Select(id));
        }
        for k in hotkeys {
            match k {
                Hotkey::Focus => self.focus_mode = !self.focus_mode,
                Hotkey::Escape => self.focus_mode = false,
            }
            ctx.request_repaint();
        }

        if self.last_poll.elapsed() < POLL {
            return;
        }
        self.last_poll = Instant::now();
        let mut changed = false;
        let mut pids = HashMap::new();
        pids.insert(metrics::TOTAL.to_string(), std::process::id());
        self.audio.clear();
        for (id, p) in &self.panels {
            pids.insert(id.clone(), p.pid);
            if p.playing_audio() {
                self.audio.insert(id.clone());
            }
            let Some(a) = self.cfg.accounts.iter_mut().find(|a| &a.id == id) else {
                continue;
            };
            if let Some(u) = p.url().filter(|u| u.starts_with("http")) {
                if a.last_url.as_deref() != Some(u.as_str()) {
                    a.last_url = Some(u);
                    changed = true;
                }
            }
            let z = p.zoom();
            if (z - a.zoom).abs() > 0.001 {
                a.zoom = z;
                changed = true;
            }
        }
        self.metrics.set_watch(pids);
        if changed {
            self.mark_dirty();
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        let (f11, esc, new) = ctx.input(|i| {
            (
                i.key_pressed(Key::F11),
                i.key_pressed(Key::Escape),
                i.modifiers.command && i.key_pressed(Key::N),
            )
        });
        if f11 {
            self.focus_mode = !self.focus_mode;
        }
        if esc && self.dialog.is_none() {
            self.focus_mode = false;
        }
        if new && self.dialog.is_none() {
            self.apply(Action::NewAccount);
        }
    }

    fn register_window(&mut self, frame: &eframe::Frame) {
        if self.window_registered {
            return;
        }
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = frame.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                self.hwnd = w.hwnd.get();
                crate::single::set_main_window(self.hwnd);
                self.window_registered = true;
            }
        }
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        let (size, maximized, close) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.inner_rect.map(|r| r.size()),
                v.maximized.unwrap_or(false),
                v.close_requested(),
            )
        });
        if maximized != self.cfg.window_maximized {
            self.cfg.window_maximized = maximized;
            self.mark_dirty();
        }
        if let (Some(s), false) = (size, maximized) {
            let s = [s.x.round(), s.y.round()];
            if s[0] > 100.0 && self.cfg.window_size != Some(s) {
                self.cfg.window_size = Some(s);
                self.mark_dirty();
            }
        }
        if close {
            self.save();
        }

        let title = match self.selected_account() {
            Some(a) if !self.focus_mode || self.cfg.layout == Layout::Tabs => {
                format!("{} — Idlerx", a.name)
            }
            _ => "Idlerx".into(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn selected_account(&self) -> Option<&Account> {
        self.cfg
            .selected
            .as_deref()
            .and_then(|id| self.cfg.account(id))
    }

    // ------------------------------------------------------------ sidebar --

    fn sidebar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
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
                if ui.add(b).on_hover_text("Ctrl+N").clicked() {
                    actions.push(Action::NewAccount);
                }
            });
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new(trf!(
                "CONTAS · {}",
                "ACCOUNTS · {}",
                self.cfg.accounts.len()
            ))
            .small()
            .color(weak(dark)),
        );
        ui.add_space(4.0);

        let footer = 58.0;
        let list_height = (ui.available_height() - footer).max(60.0);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for id in self.cfg.display_order() {
                    if let Some(a) = self.cfg.account(&id) {
                        self.account_row(ui, a, actions);
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
                    .on_hover_text(tr!("Configurações", "Settings"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::Settings);
                }
                ui.label(
                    RichText::new(format!("v{}", update::VERSAO_ATUAL))
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
                    let text = trf!(
                        "Total: {:.0}% · {}",
                        "Total: {:.0}% · {}",
                        total.cpu,
                        metrics::fmt_memory(total.memory)
                    );
                    let detail = trf!(
                        "Contas abertas ({n}): {cpu:.0}% CPU · {}
Interface do Idlerx: {:.0}% CPU · {}",
                        "Open accounts ({n}): {cpu:.0}% CPU · {}
Idlerx interface: {:.0}% CPU · {}",
                        metrics::fmt_memory(mem),
                        (total.cpu - cpu).max(0.0),
                        metrics::fmt_memory(total.memory.saturating_sub(mem))
                    );
                    ui.label(RichText::new(text).small().color(weak(dark)))
                        .on_hover_text(detail);
                });
            });
            ui.separator();
        });
    }

    fn status_text(&self, id: &str) -> (String, Color32, bool) {
        if let Some(p) = self.panels.get(id) {
            let mut s = metrics::fmt_duration(p.started.elapsed());
            if let Some(u) = self.metrics.get(id) {
                s.push_str(&format!(
                    " · {:.0}% · {}",
                    u.cpu,
                    metrics::fmt_memory(u.memory)
                ));
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

    fn account_row(&self, ui: &mut egui::Ui, a: &Account, actions: &mut Vec<Action>) {
        let dark = ui.visuals().dark_mode;
        let selected = self.cfg.selected.as_deref() == Some(a.id.as_str());
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ROW_HEIGHT),
            Sense::click_and_drag(),
        );
        resp.dnd_set_drag_payload(DragAccount(a.id.clone()));
        let drop_here = resp
            .dnd_hover_payload::<DragAccount>()
            .is_some_and(|p| p.0 != a.id);
        if let Some(p) = resp.dnd_release_payload::<DragAccount>() {
            actions.push(Action::MoveTo(p.0.clone(), a.id.clone()));
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
        if drop_here {
            painter.line_segment(
                [
                    rect.left_top() + Vec2::new(4.0, 1.0),
                    rect.right_top() + Vec2::new(-4.0, 1.0),
                ],
                Stroke::new(2.0, ACCENT),
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

        let text_left = rect.left() + 50.0;
        let right_reserved = 50.0;
        let max_w = rect.right() - text_left - right_reserved;
        single_line(
            &painter,
            Pos2::new(text_left, rect.top() + 8.0),
            &a.name,
            FontId::proportional(14.5),
            ui.visuals().strong_text_color(),
            max_w,
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
            FontId::proportional(11.5),
            weak(dark),
            max_w,
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
        if resp.hovered() || ui.rect_contains_pointer(rect) {
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
            let b = ui
                .put(r, egui::Button::new(icon).small())
                .on_hover_text(hint);
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

        let resp = resp.on_hover_text(&a.url);
        resp.context_menu(|ui| self.account_menu(ui, a, open, actions));
    }

    fn account_menu(&self, ui: &mut egui::Ui, a: &Account, open: bool, actions: &mut Vec<Action>) {
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

    // ------------------------------------------------------------ toolbar --

    fn toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
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
                .on_hover_text(hint)
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
                    .on_hover_text(tr!(
                        "Modo foco: só os jogos na tela (F11)",
                        "Focus mode: only the games on screen (F11)"
                    ))
                    .clicked()
                {
                    self.focus_mode = true;
                }
                self.layout_switch(ui);

                if let Some(n) = update::disponivel() {
                    let t = RichText::new(trf!("⬆ Versão {n}", "⬆ Version {n}")).color(ACCENT);
                    if ui
                        .link(t)
                        .on_hover_text(tr!("Nova versão disponível", "New version available"))
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
                        .on_hover_text(hint)
                        .clicked()
                    {
                        actions.push(Action::ToggleMute(id.clone()));
                    }
                    if panel_open {
                        let zoom = a.zoom;
                        let set = |z: f64, me: &Self| {
                            if let Some(p) = me.panels.get(&id) {
                                p.set_zoom((z * 10.0).round() / 10.0);
                            }
                        };
                        if ui
                            .add(egui::Button::new("+").frame(false))
                            .on_hover_text(tr!("Aumentar zoom", "Zoom in"))
                            .clicked()
                        {
                            set(zoom + 0.1, self);
                            self.last_poll = Instant::now() - POLL;
                        }
                        if ui
                            .add(
                                egui::Button::new(format!("{:.0}%", zoom * 100.0))
                                    .frame(false)
                                    .min_size(Vec2::new(44.0, 0.0)),
                            )
                            .on_hover_text(tr!(
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
                            .on_hover_text(tr!("Diminuir zoom", "Zoom out"))
                            .clicked()
                        {
                            set(zoom - 0.1, self);
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

    fn layout_switch(&mut self, ui: &mut egui::Ui) {
        let before = (self.cfg.layout, self.cfg.grid_columns);
        if self.cfg.layout == Layout::Grid {
            let label = match self.cfg.grid_columns {
                0 => tr!("Auto", "Auto").to_string(),
                n => trf!("{n} col.", "{n} col."),
            };
            egui::ComboBox::from_id_salt("columns")
                .selected_text(label)
                .width(70.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.cfg.grid_columns,
                        0,
                        tr!("Automático", "Automatic"),
                    );
                    for n in 1..=6u8 {
                        ui.selectable_value(
                            &mut self.cfg.grid_columns,
                            n,
                            trf!("{n} coluna(s)", "{n} column(s)"),
                        );
                    }
                })
                .response
                .on_hover_text(tr!("Colunas da grade", "Grid columns"));
        }
        ui.selectable_value(&mut self.cfg.layout, Layout::Grid, tr!("⊞ Grade", "⊞ Grid"))
            .on_hover_text(tr!(
                "Todas as contas abertas lado a lado",
                "All open accounts side by side"
            ));
        ui.selectable_value(&mut self.cfg.layout, Layout::Tabs, tr!("▣ Uma", "▣ Single"))
            .on_hover_text(tr!("Uma conta por vez", "One account at a time"));
        if before != (self.cfg.layout, self.cfg.grid_columns) {
            self.mark_dirty();
        }
    }

    // ------------------------------------------------------------ content --

    fn content(
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
            centered(ui, rect, |ui| {
                ui.label(RichText::new("⚠").size(36.0).color(FAVORITE));
                ui.label(
                    RichText::new(tr!(
                        "O WebView2 Runtime não foi encontrado",
                        "WebView2 Runtime was not found"
                    ))
                    .size(18.0)
                    .strong(),
                );
                ui.label(tr!(
                    "O Idlerx usa o motor do Microsoft Edge (WebView2) para abrir as contas. \
                     Ele já vem no Windows 11; instale pelo site oficial da Microsoft e abra o Idlerx de novo.",
                    "Idlerx uses the Microsoft Edge engine (WebView2) to open the accounts. \
                     It ships with Windows 11; install it from Microsoft's official site and open Idlerx again."
                ));
                ui.label(RichText::new(e).small().color(weak(dark)));
                if ui
                    .button(tr!(
                        "Baixar o WebView2 Runtime",
                        "Download WebView2 Runtime"
                    ))
                    .clicked()
                {
                    let _ = open::that(panels::RUNTIME_DOWNLOAD_URL);
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
                    actions.push(Action::NewAccount);
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
                    placements.insert(a.id.clone(), rect);
                } else {
                    self.placeholder(ui, rect, &a, actions);
                }
            }
            Layout::Grid => self.grid(ui, rect, placements, actions),
        }
    }

    /// Area of a closed, opening or failed account.
    fn placeholder(&self, ui: &mut egui::Ui, rect: Rect, a: &Account, actions: &mut Vec<Action>) {
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
                ui.spinner();
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

    fn grid(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        placements: &mut HashMap<String, Rect>,
        actions: &mut Vec<Action>,
    ) {
        let dark = ui.visuals().dark_mode;
        let ids: Vec<String> = self
            .cfg
            .display_order()
            .into_iter()
            .filter(|id| {
                self.cfg.account(id).is_some_and(|a| a.in_grid) && self.is_open_or_opening(id)
            })
            .collect();
        if ids.is_empty() {
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
                actions.push(Action::MoveTo(p.0.clone(), p.0.clone()));
            }
            return;
        }

        let area = rect.shrink(GRID_GAP);
        let header = if self.focus_mode { 0.0 } else { CELL_HEADER };
        let cols = if self.cfg.grid_columns > 0 {
            (self.cfg.grid_columns as usize).min(ids.len())
        } else {
            auto_columns(ids.len(), area.size(), header)
        };
        let rows = ids.len().div_ceil(cols);
        let cell = Vec2::new(
            (area.width() - GRID_GAP * (cols as f32 - 1.0)) / cols as f32,
            (area.height() - GRID_GAP * (rows as f32 - 1.0)) / rows as f32,
        );

        for (i, id) in ids.iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            let min = area.min
                + Vec2::new(
                    c as f32 * (cell.x + GRID_GAP),
                    r as f32 * (cell.y + GRID_GAP),
                );
            let cell_rect = Rect::from_min_size(min, cell);
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

            let stroke = if hovering {
                Stroke::new(2.0, ACCENT)
            } else if selected && ids.len() > 1 {
                Stroke::new(1.5, ACCENT)
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
                placements.insert(id.clone(), page.shrink2(Vec2::new(1.0, 1.0)));
            } else {
                self.placeholder(ui, page, &a, actions);
            }
        }
    }

    fn cell_header(&self, ui: &mut egui::Ui, rect: Rect, a: &Account, actions: &mut Vec<Action>) {
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

        let mut usage = String::new();
        if let (Some(p), Some(u)) = (self.panels.get(&a.id), self.metrics.get(&a.id)) {
            usage = format!(
                "{} · {:.0}% · {}",
                metrics::fmt_duration(p.started.elapsed()),
                u.cpu,
                metrics::fmt_memory(u.memory)
            );
        }

        // buttons from the right
        let mut x = rect.right() - 4.0;
        let mut button = |icon: &str, hint: &str| -> bool {
            let r = Rect::from_min_size(
                Pos2::new(x - 24.0, rect.top() + 2.0),
                Vec2::new(24.0, rect.height() - 4.0),
            );
            x -= 26.0;
            ui.put(r, egui::Button::new(icon).frame(false))
                .on_hover_text(hint)
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

        let usage_w = if usage.is_empty() || rect.width() < 360.0 {
            0.0
        } else {
            150.0
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

    // ------------------------------------------------------------ dialogs --

    fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
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
            Dialog::Settings => {
                if self.settings_dialog(ctx) {
                    keep = false;
                }
                Dialog::Settings
            }
            Dialog::Update => {
                if update_dialog(ctx) {
                    keep = false;
                    update::limpar_resultado();
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
                        "Login, cookies e cache desta conta serão apagados: ela volta como nova e você precisará entrar no jogo de novo.",
                        "Login, cookies and cache of this account will be deleted: it starts fresh and you will need to log in to the game again."
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

    fn save_account(
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
    fn settings_dialog(&mut self, ctx: &egui::Context) -> bool {
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
            ui.checkbox(
                &mut self.cfg.check_updates,
                tr!(
                    "Verificar novas versões automaticamente",
                    "Check for new versions automatically"
                ),
            );
            ui.horizontal(|ui| {
                if ui.button(tr!("Verificar agora", "Check now")).clicked() {
                    update::verificar_agora();
                }
                match update::estado() {
                    update::Estado::Verificando => {
                        ui.spinner();
                    }
                    update::Estado::EmDia => {
                        ui.label(
                            RichText::new(tr!(
                                "✔ Você tem a versão mais recente.",
                                "✔ You have the latest version."
                            ))
                            .color(ONLINE),
                        );
                    }
                    update::Estado::FalhaVerificacao(e) => {
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    _ => {
                        if let Some(n) = update::disponivel() {
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
                if ui
                    .button(tr!("📁 Pasta dos dados", "📁 Data folder"))
                    .clicked()
                {
                    let dir = config::data_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = open::that(dir);
                }
                if ui
                    .button(tr!("📁 Perfis das contas", "📁 Account profiles"))
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
                    RichText::new(format!("Idlerx v{}", update::VERSAO_ATUAL)).color(weak(dark)),
                );
                if ui
                    .link(tr!("código-fonte (GPL-3.0)", "source code (GPL-3.0)"))
                    .clicked()
                {
                    let _ = open::that(update::pagina_repositorio());
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
            i18n::aplicar(
                self.cfg
                    .language
                    .as_deref()
                    .and_then(i18n::Idioma::do_codigo),
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

    fn after_update_check(&mut self, ctx: &egui::Context) {
        if matches!(update::estado(), update::Estado::Reiniciando(_)) && !self.closing {
            self.closing = true;
            self.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.register_window(frame);
        self.after_update_check(&ctx);
        self.open_next(&ctx);
        self.poll(&ctx);
        self.keyboard(&ctx);
        self.track_window(&ctx);

        let dark = ui.visuals().dark_mode;
        let mut actions = Vec::new();
        let mut placements = HashMap::new();

        if self.focus_mode {
            egui::Panel::top("focus_bar")
                .exact_size(22.0)
                .frame(
                    egui::Frame::new()
                        .fill(panel_bg(dark))
                        .inner_margin(Margin::symmetric(8, 0)),
                )
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(
                            RichText::new(tr!(
                                "Modo foco · F11 ou Esc para sair",
                                "Focus mode · F11 or Esc to leave"
                            ))
                            .small()
                            .color(weak(dark)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(tr!(
                                            "⛶ Sair do modo foco",
                                            "⛶ Leave focus mode"
                                        ))
                                        .small(),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.focus_mode = false;
                            }
                        });
                    });
                });
        } else {
            egui::Panel::left("sidebar")
                .exact_size(SIDEBAR_WIDTH)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(panel_bg(dark))
                        .inner_margin(Margin::symmetric(10, 0)),
                )
                .show(ui, |ui| self.sidebar(ui, &mut actions));
            egui::Panel::top("toolbar")
                .exact_size(44.0)
                .frame(
                    egui::Frame::new()
                        .fill(panel_bg(dark))
                        .inner_margin(Margin::symmetric(10, 0)),
                )
                .show(ui, |ui| self.toolbar(ui, &mut actions));
        }
        egui::CentralPanel::no_frame()
            .show(ui, |ui| self.content(ui, &mut placements, &mut actions));
        self.dialogs(&ctx, &mut actions);

        for a in actions {
            self.apply(a);
        }

        // pages always cover what egui paints: hide the ones under a menu,
        // drop-down or dialog while it is open
        let overlays = overlay_rects(&ctx);
        let ppp = ctx.pixels_per_point();
        for (id, p) in &mut self.panels {
            let rect = placements
                .get(id)
                .copied()
                .filter(|r| self.dialog.is_none() && !overlays.iter().any(|o| o.intersects(*r)));
            p.place(rect, ppp);
        }

        // a click outside the pages gives the keyboard back to egui
        if ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(pos) = ctx.pointer_interact_pos() {
                if !placements.values().any(|r| r.contains(pos)) {
                    if let Some(p) = self.panels.values().next() {
                        p.focus_parent();
                    }
                }
            }
        }

        if self.dirty && self.changed_at.elapsed() >= SAVE_DELAY {
            self.save();
        }
        ctx.request_repaint_after(POLL);
    }

    fn on_exit(&mut self) {
        self.save();
        // the pages are closed before the window goes away
        self.panels.clear();
    }
}

/// Returns true when it should close.
fn update_dialog(ctx: &egui::Context) -> bool {
    let modal = egui::Modal::new(egui::Id::new("update")).show(ctx, |ui| {
        ui.set_width(420.0);
        let mut close = false;
        match update::estado() {
            update::Estado::Disponivel(v) => {
                ui.heading(trf!("Versão {} disponível", "Version {} available", v.numero));
                ui.label(trf!("Você está na versão {}.", "You are on version {}.", update::VERSAO_ATUAL));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if v.instalavel()
                        && ui.add(egui::Button::new(RichText::new(tr!("Atualizar agora", "Update now")).color(Color32::WHITE)).fill(ACCENT)).clicked() {
                            update::instalar(v.clone());
                        }
                    if ui.button(tr!("Ver novidades", "What's new")).clicked() {
                        let _ = open::that(&v.pagina);
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
            update::Estado::Baixando(v, f) => {
                ui.heading(trf!("Baixando a versão {}…", "Downloading version {}…", v.numero));
                ui.add(egui::ProgressBar::new(f).show_percentage());
            }
            update::Estado::FalhaInstalacao(v, e) => {
                ui.heading(tr!("Não foi possível atualizar", "Could not update"));
                ui.label(RichText::new(e).color(DANGER));
                ui.horizontal(|ui| {
                    if ui.button(tr!("Tentar de novo", "Try again")).clicked() {
                        update::instalar(v.clone());
                    }
                    if ui.button(tr!("Baixar pela página", "Download from the page")).clicked() {
                        let _ = open::that(&v.pagina);
                    }
                    if ui.button(tr!("Fechar", "Close")).clicked() {
                        close = true;
                    }
                });
            }
            update::Estado::Reiniciando(_) => {
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

// ----------------------------------------------------------------- helpers --

/// Areas egui draws above everything (menus, drop-downs, modal dialogs).
/// Tooltips are left out: hiding a page for a tooltip would only flicker.
fn overlay_rects(ctx: &egui::Context) -> Vec<Rect> {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Foreground)
            .filter_map(|l| m.area_rect(l.id))
            .collect()
    })
}

/// Number of grid columns that gives the largest cells closest to 16:9.
fn auto_columns(n: usize, area: Vec2, header: f32) -> usize {
    if n <= 1 {
        return 1;
    }
    let mut best = (1, f32::MIN);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let w = (area.x - GRID_GAP * (cols as f32 - 1.0)) / cols as f32;
        let h = (area.y - GRID_GAP * (rows as f32 - 1.0)) / rows as f32 - header;
        // usable 16:9 area inside the cell
        let score = w.min(h * 16.0 / 9.0);
        if score > best.1 + 0.5 {
            best = (cols, score);
        }
    }
    best.0
}

fn apply_style(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::Light => egui::ThemePreference::Light,
    });
    for t in [egui::Theme::Dark, egui::Theme::Light] {
        let text = if t == egui::Theme::Dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        ctx.style_mut_of(t, |s| {
            s.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
            s.visuals.selection.stroke = Stroke::new(1.0, text);
            s.visuals.hyperlink_color = ACCENT;
            s.spacing.button_padding = Vec2::new(8.0, 4.0);
            s.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
            s.visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
            s.visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
            s.visuals.widgets.active.corner_radius = CornerRadius::same(6);
        });
    }
}

fn panel_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x18, 0x18, 0x1b)
    } else {
        Color32::from_rgb(0xf4, 0xf4, 0xf5)
    }
}

fn content_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x0f, 0x0f, 0x12)
    } else {
        Color32::from_rgb(0xe4, 0xe4, 0xe7)
    }
}

fn border(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2a, 0x2a, 0x30)
    } else {
        Color32::from_rgb(0xd4, 0xd4, 0xd8)
    }
}

fn weak(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x9a, 0x9a, 0xa3)
    } else {
        Color32::from_rgb(0x63, 0x63, 0x6b)
    }
}

/// Avatar color, stable per account.
fn avatar_color(id: &str) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(0x0e, 0xa5, 0xe9),
        Color32::from_rgb(0x8b, 0x5c, 0xf6),
        Color32::from_rgb(0xec, 0x48, 0x99),
        Color32::from_rgb(0xf9, 0x73, 0x16),
        Color32::from_rgb(0x10, 0xb9, 0x81),
        Color32::from_rgb(0x63, 0x66, 0xf1),
        Color32::from_rgb(0xe1, 0x1d, 0x48),
        Color32::from_rgb(0x0d, 0x94, 0x88),
    ];
    let h = id
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    COLORS[h as usize % COLORS.len()]
}

fn avatar(painter: &egui::Painter, center: Pos2, radius: f32, a: &Account, active: bool) {
    let mut c = avatar_color(&a.id);
    if !active {
        c = c.gamma_multiply(0.55);
    }
    painter.circle_filled(center, radius, c);
    painter.text(
        center,
        Align2::CENTER_CENTER,
        config::initials(&a.name),
        FontId::proportional(radius * 0.85),
        Color32::WHITE,
    );
}

/// Text in one line, cut with "…" when it does not fit.
fn single_line(
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    painter.galley(pos, galley, color);
}

/// Content centered in `rect`.
fn centered(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui)) {
    let width = (rect.width() - 40.0).clamp(10.0, 460.0);
    let inner = Rect::from_center_size(rect.center(), Vec2::new(width, rect.height().min(320.0)));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(Align::Center)),
    );
    child.spacing_mut().item_spacing.y = 6.0;
    add(&mut child);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colunas_automaticas() {
        let wide = Vec2::new(1600.0, 900.0);
        assert_eq!(auto_columns(1, wide, 28.0), 1);
        assert_eq!(auto_columns(2, wide, 28.0), 2);
        assert_eq!(auto_columns(4, wide, 28.0), 2);
        let tall = Vec2::new(900.0, 1600.0);
        assert_eq!(auto_columns(2, tall, 28.0), 1);
        // never more columns than accounts
        assert!(auto_columns(3, Vec2::new(5000.0, 200.0), 28.0) <= 3);
    }

    #[test]
    fn cor_do_avatar_estavel() {
        assert_eq!(avatar_color("abc"), avatar_color("abc"));
    }
}
