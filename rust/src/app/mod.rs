//! User interface: account list on the left, navigation bar on top and the
//! open accounts (one at a time or in a grid) in the rest of the window.
//!
//! The pages are native child windows placed over the egui surface, so they
//! always cover anything egui paints in their area: pages under a menu or a
//! dialog are hidden for the moment, and tooltips open where no page is.
//!
//! - `sidebar`: accounts, groups, drag and drop, totals
//! - `toolbar`: navigation, address, zoom, sound, layout and grid shape
//! - `grid`: one account or the grid, placeholders, cell headers
//! - `dialogs`: account, group, settings, update, confirmations
//! - `network`: VPN of groups (tunnels, waiting accounts, group dialog part)
//! - `widgets`: colors, avatar, text, tooltips, small drawings

use crate::config::{self, Account, Config, Group, Layout, Theme};
use crate::metrics::{self, Metrics};
use crate::panels::{self, Hotkey, Panel};
use crate::{i18n, update, vpn};
use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Color32, CornerRadius, FontId, Key, Margin, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, TextFormat, Vec2,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod dialogs;
mod grid;
mod network;
mod sidebar;
mod toolbar;
mod widgets;

use widgets::*;

const ACCENT: Color32 = Color32::from_rgb(0x14, 0xb8, 0xa6);
const FAVORITE: Color32 = Color32::from_rgb(0xf5, 0xb3, 0x01);
const ONLINE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
const DANGER: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
const SIDEBAR_WIDTH: f32 = 272.0;
const ROW_HEIGHT: f32 = 52.0;
const GROUP_HEIGHT: f32 = 30.0;
/// Indentation of the accounts inside a group.
const GROUP_INDENT: f32 = 10.0;
const CELL_HEADER: f32 = 28.0;
const GRID_GAP: f32 = 4.0;
/// Repaint and page polling while the window is on screen (the numbers
/// change every 2 s; clicks, keys and page events repaint at once).
const POLL: Duration = Duration::from_secs(2);
/// Same while minimized: nothing is drawn, only the bookkeeping runs.
const POLL_HIDDEN: Duration = Duration::from_secs(5);
const SAVE_DELAY: Duration = Duration::from_secs(2);

static CHECK_UPDATES: AtomicBool = AtomicBool::new(true);

fn check_updates_enabled() -> bool {
    CHECK_UPDATES.load(Ordering::Relaxed)
}

/// Payload of an account dragged from the list.
#[derive(Clone)]
struct DragAccount(String);

/// Payload of a group dragged in the list.
#[derive(Clone)]
struct DragGroup(String);

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
    /// Reorders in the side list (the account is not opened).
    Reorder(String, String),
    /// Opens the account and puts it in the grid right after another one
    /// (an empty grid cell); None = the grid is empty.
    PlaceAfter(String, Option<String>),
    MoveToGroup(String, Option<String>),
    /// Asks the name of a new group; the account (if any) goes into it.
    NewGroup(Option<String>),
    RenameGroup(String),
    DeleteGroup(String),
    /// Moves the first group to the place of the second one.
    ReorderGroup(String, String),
    /// Moves a group one place up (-1) or down (+1).
    MoveGroup(String, i32),
    /// Frames per second of one account (None = automatic).
    SetFps(String, Option<u32>),
    ToggleCollapse(String),
    OpenGroup(String),
    PauseGroup(String),
    /// Grid shows only this group (None = all accounts).
    GridGroup(Option<String>),
    /// Connects a group's VPN again (after a failure).
    RetryVpn(String),
    InstallOpenVpn,
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

struct GroupEdit {
    /// None = new group.
    id: Option<String>,
    name: String,
    /// Account that goes into the new group.
    assign: Option<String>,
    vpn: network::VpnEdit,
    error: Option<String>,
}

enum Dialog {
    Edit(EditState),
    Group(GroupEdit),
    Settings,
    Update,
    ConfirmRemove(String),
    ConfirmClear(String),
    ConfirmQuit,
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
    /// Economy mode and background setting in effect (they apply to the
    /// shared browser, so only change on restart).
    economy: bool,
    keep_active: bool,
    /// "Restart now" was clicked.
    restart: bool,
    /// The window was shown (maximized when it was closed maximized).
    window_shown: bool,
    created: Instant,
    /// IDLERX_SCREENSHOT: opens on a given screen (documentation pictures).
    screenshot: Option<String>,
    /// Theme last applied to the pages (setting, dark).
    page_theme: Option<(Theme, bool)>,
    /// Economy mode: main frame id -> page processes.
    renderers: Rc<RefCell<panels::Renderers>>,
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
    /// VPN of each group (and its gate).
    tunnels: vpn::Tunnels,
    /// Last tunnel "epoch" seen per group (a new one = it came back).
    vpn_epochs: HashMap<String, u64>,
    installing: Option<network::Install>,
    install_result: Option<Result<bool, String>>,
    /// Accounts reopened with another network: attempts and when to try.
    retry_open: HashMap<String, (u8, Instant)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config) -> App {
        let ctx = cc.egui_ctx.clone();
        apply_style(&ctx, cfg.theme);

        let repaint = ctx.clone();
        let metrics = Metrics::start(move || repaint.request_repaint());

        let repaint = ctx.clone();
        update::on_change(move || repaint.request_repaint());
        CHECK_UPDATES.store(cfg.check_updates, Ordering::Relaxed);
        update::start_periodic_check(check_updates_enabled);

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

        let (economy, keep_active) = (cfg.economy, cfg.keep_active);
        let repaint = ctx.clone();
        let tunnels = vpn::Tunnels::new(Arc::new(move || repaint.request_repaint()));
        let screenshot = std::env::var("IDLERX_SCREENSHOT")
            .ok()
            .filter(|v| !v.is_empty());
        let dialog = match screenshot.as_deref() {
            Some("settings") => Some(Dialog::Settings),
            // the first group's dialog, with its VPN
            Some("group") => cfg.groups.first().map(|g| {
                Dialog::Group(GroupEdit {
                    id: Some(g.id.clone()),
                    name: g.name.clone(),
                    assign: None,
                    vpn: network::VpnEdit::of(Some(g)),
                    error: None,
                })
            }),
            _ => None,
        };
        App {
            cfg,
            panels: HashMap::new(),
            failed: HashMap::new(),
            queue,
            starting: None,
            hwnd: 0,
            economy,
            keep_active,
            restart: false,
            page_theme: None,
            screenshot,
            window_shown: false,
            created: Instant::now(),
            renderers: Rc::new(RefCell::new(HashMap::new())),
            metrics,
            runtime: panels::runtime_version(),
            address: String::new(),
            address_owner: None,
            address_error: false,
            focus_mode: false,
            dialog,
            window_registered: false,
            last_poll: Instant::now(),
            audio: HashSet::new(),
            dirty: false,
            changed_at: Instant::now(),
            closing: false,
            window_title: String::new(),
            tunnels,
            vpn_epochs: HashMap::new(),
            installing: None,
            install_result: None,
            retry_open: HashMap::new(),
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
                    p.set_theme(self.cfg.theme, Self::dark(ctx));
                    self.retry_open.remove(&id);
                    self.panels.insert(id, p);
                }
                Ok(_) => {} // removed meanwhile: the page is closed here
                Err(e) => match self.retry_open.get_mut(&id) {
                    // reopened with another network: the previous browser
                    // may still be letting its folder go
                    Some((n, at)) if *n < 8 => {
                        *n += 1;
                        *at = Instant::now() + Duration::from_secs(1);
                        self.queue.push_back(id);
                    }
                    _ => {
                        self.retry_open.remove(&id);
                        self.failed.insert(id, e);
                    }
                },
            }
            ctx.request_repaint();
        }

        // the first account that can open now: accounts of a group with a
        // VPN wait for it, without holding the others back
        let now = Instant::now();
        let ready = self.queue.iter().find(|id| {
            self.network_ready(id)
                && self
                    .retry_open
                    .get(id.as_str())
                    .is_none_or(|(_, at)| *at <= now)
        });
        let Some(id) = ready.cloned() else {
            if !self.retry_open.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(300));
            }
            return;
        };
        match self.cfg.account(&id).cloned() {
            Some(acc) if !self.panels.contains_key(&id) => {
                let gate = self.vpn_gate(&id);
                let repaint = ctx.clone();
                let wake: panels::Wake = Rc::new(move || repaint.request_repaint());
                let browser = panels::Browser {
                    keep_active: if self.economy {
                        self.keep_active
                    } else {
                        self.cfg.keep_active
                    },
                    economy: self.economy,
                    vpn: gate.as_ref().map(|(g, port)| (g.as_str(), *port)),
                };
                let slot = panels::open(
                    self.hwnd,
                    &acc,
                    browser,
                    self.cfg.save_passwords,
                    Self::dark(ctx),
                    wake,
                );
                self.starting = Some((id, slot));
            }
            _ => {
                self.queue.retain(|q| q != &id);
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
                let before = self.vpn_of(&id);
                self.cfg.move_to(&id, &target);
                if self.vpn_of(&id) != before {
                    self.restart_account(&id);
                }
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.in_grid = true;
                }
                self.request_open(&id);
                self.cfg.selected = Some(id);
                self.mark_dirty();
            }
            Action::Reorder(id, target) => {
                let before = self.vpn_of(&id);
                self.cfg.move_to(&id, &target);
                if self.vpn_of(&id) != before {
                    self.restart_account(&id);
                }
                self.mark_dirty();
            }
            Action::PlaceAfter(id, after) => {
                let before = self.vpn_of(&id);
                match &after {
                    Some(t) => self.cfg.move_after(&id, t),
                    None => {
                        // empty grid showing one group: the account joins it
                        if let Some(g) = self.cfg.grid_group.clone() {
                            self.cfg.move_to_group(&id, Some(&g));
                        }
                    }
                }
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.in_grid = true;
                }
                if self.vpn_of(&id) != before {
                    self.restart_account(&id);
                }
                self.request_open(&id);
                self.cfg.selected = Some(id);
                self.mark_dirty();
            }
            Action::MoveToGroup(id, group) => {
                let before = self.vpn_of(&id);
                self.cfg.move_to_group(&id, group.as_deref());
                if self.vpn_of(&id) != before {
                    self.restart_account(&id);
                }
                self.mark_dirty();
            }
            Action::NewGroup(assign) => {
                self.dialog = Some(Dialog::Group(GroupEdit {
                    id: None,
                    name: String::new(),
                    assign,
                    vpn: network::VpnEdit::default(),
                    error: None,
                }))
            }
            Action::RenameGroup(gid) => {
                if let Some(g) = self.cfg.group(&gid) {
                    self.dialog = Some(Dialog::Group(GroupEdit {
                        id: Some(gid.clone()),
                        name: g.name.clone(),
                        assign: None,
                        vpn: network::VpnEdit::of(Some(g)),
                        error: None,
                    }));
                }
            }
            Action::DeleteGroup(gid) => {
                let had_vpn = self
                    .cfg
                    .group(&gid)
                    .is_some_and(|g| g.active_vpn().is_some());
                let ids = self.cfg.accounts_in(Some(&gid));
                self.cfg.delete_group(&gid);
                self.forget_group_vpn(&gid);
                if had_vpn {
                    // its accounts leave the VPN
                    for id in ids {
                        self.restart_account(&id);
                    }
                }
                self.mark_dirty();
            }
            Action::RetryVpn(gid) => {
                if let Some(v) = self.cfg.group(&gid).and_then(|g| g.active_vpn()).cloned() {
                    self.tunnels.retry(&gid, &v);
                }
            }
            Action::InstallOpenVpn => self.install_openvpn(),
            Action::ToggleCollapse(gid) => {
                if let Some(g) = self.cfg.group_mut(&gid) {
                    g.collapsed = !g.collapsed;
                    self.mark_dirty();
                }
            }
            Action::OpenGroup(gid) => {
                for id in self.cfg.accounts_in(Some(&gid)) {
                    self.request_open(&id);
                }
            }
            Action::PauseGroup(gid) => {
                for id in self.cfg.accounts_in(Some(&gid)) {
                    self.close_panel(&id);
                }
            }
            Action::GridGroup(g) => {
                self.cfg.grid_group = g;
                self.cfg.layout = Layout::Grid;
                self.mark_dirty();
            }
            Action::ReorderGroup(id, target) => {
                self.cfg.move_group_to(&id, &target);
                self.mark_dirty();
            }
            Action::MoveGroup(id, delta) => {
                self.cfg.move_group_by(&id, delta);
                self.mark_dirty();
            }
            Action::SetFps(id, fps) => {
                if let Some(a) = self.cfg.account_mut(&id) {
                    a.fps = fps;
                    self.mark_dirty();
                }
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
        pids.insert(metrics::TOTAL.to_string(), vec![std::process::id()]);
        self.audio.clear();
        if self.economy {
            // which page processes belong to each account (answer arrives
            // asynchronously and is used on the next round), asked once per
            // shared browser (groups with a VPN have their own)
            let mut asked = HashSet::new();
            for p in self.panels.values() {
                if asked.insert(p.pid) {
                    let repaint = ctx.clone();
                    p.request_renderers(self.renderers.clone(), move || repaint.request_repaint());
                }
            }
        }
        let renderers = self.renderers.borrow().clone();
        for (id, p) in &self.panels {
            let roots = if self.economy {
                p.main_frame_id()
                    .and_then(|f| renderers.get(&(p.pid, f)).cloned())
                    .unwrap_or_default()
            } else {
                vec![p.pid]
            };
            pids.insert(id.clone(), roots);
            p.request_fps_rate();
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

    /// Frame limit of each page: the selected account draws freely, the
    /// others at the configured limit (or each account's own choice).
    fn apply_fps(&self) {
        for (id, p) in &self.panels {
            if let Some(a) = self.cfg.account(id) {
                let selected = self.cfg.selected.as_deref() == Some(id.as_str());
                p.set_fps(self.cfg.fps_for(a, selected));
            }
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        if self.window_shown {
            return;
        }
        self.window_shown = true;
        if self.cfg.window_maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        let (size, maximized, minimized, close) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.inner_rect.map(|r| r.size()),
                v.maximized.unwrap_or(false),
                v.minimized.unwrap_or(false),
                v.close_requested(),
            )
        });
        // the first moments still show the window being created/maximized
        let settled = self.created.elapsed() >= Duration::from_secs(2) && !minimized;
        if settled && maximized != self.cfg.window_maximized {
            self.cfg.window_maximized = maximized;
            self.mark_dirty();
        }
        if let (Some(s), false, true) = (size, maximized, settled) {
            let s = [s.x.round(), s.y.round()];
            if s[0] > 100.0 && self.cfg.window_size != Some(s) {
                self.cfg.window_size = Some(s);
                self.mark_dirty();
            }
        }
        if close {
            if self.closing {
                self.save();
            } else {
                // ask first; the dialog closes for real (see quit())
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if minimized {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.dialog = Some(Dialog::ConfirmQuit);
            }
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

    /// Closes the app without asking (confirmed, restart or update).
    pub(super) fn quit(&mut self, ctx: &egui::Context) {
        self.closing = true;
        self.save();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Opens a new instance (it waits for this one to close) and closes.
    fn restart_now(&mut self, ctx: &egui::Context) {
        self.restart = false;
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let started = std::process::Command::new(exe)
            .arg(update::ARG_AFTER_UPDATE)
            .arg(std::process::id().to_string())
            .spawn();
        if started.is_ok() {
            self.quit(ctx);
        }
    }

    fn after_update_check(&mut self, ctx: &egui::Context) {
        if self.restart && !self.closing {
            self.dialog = None;
            self.restart_now(ctx);
            return;
        }
        if matches!(update::state(), update::State::Restarting(_)) && !self.closing {
            self.quit(ctx);
        }
    }
}

impl eframe::App for App {
    /// Work that draws nothing; it also runs while the window is minimized.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.register_window(frame);
        self.after_update_check(ctx);
        self.sync_vpn();
        self.open_next(ctx);
        self.poll(ctx);
        self.show_window(ctx);
        self.apply_fps();

        // minimized: every page stops drawing and keeps less memory (the
        // games keep running); ui() places them again when restored
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized {
            for p in self.panels.values_mut() {
                p.place(None, 1.0);
            }
            ctx.request_repaint_after(POLL_HIDDEN);
        }

        if self.dirty && self.changed_at.elapsed() >= SAVE_DELAY {
            self.save();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.keyboard(&ctx);
        self.track_window(&ctx);

        let dark = ui.visuals().dark_mode;
        // the app's theme prevails over Windows' in the pages too (also when
        // "System" and Windows switches between light and dark)
        let theme = (self.cfg.theme, Self::dark(&ctx));
        if self.page_theme != Some(theme) {
            self.page_theme = Some(theme);
            for p in self.panels.values() {
                p.set_theme(theme.0, theme.1);
            }
        }
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
        self.drag_ghost(&ctx);

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

        ctx.request_repaint_after(POLL);
    }

    fn on_exit(&mut self) {
        self.save();
        // the pages are closed before the window goes away
        self.panels.clear();
    }
}
