//! The browser engine: Firefox, controlled through Idlerx's extension.
//!
//! One Firefox per network: `main` (accounts without a VPN) and one per
//! group with a VPN (its proxy is fixed for the whole browser). Each account
//! is a container of that Firefox (its own cookies, logins and storage) shown
//! in a window of its own, which Idlerx embeds in the main window.
//!
//! The extension talks to Idlerx through native messaging: Firefox starts
//! `Idlerx.exe` as its native app, which relays everything to this process
//! through a local socket (see `host`). Commands go to the extension (open,
//! navigate, zoom, sound, hidden, recording, picture), events come back
//! (address, title, loading, sound, recorded input, keys, logins).
//!
//! Every Firefox process is in a job object that ends with Idlerx: no
//! Firefox is ever left behind.

pub mod host;
pub mod profile;
mod rdp;
pub mod window;

use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
pub use window::Window;

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Oldest Firefox that has everything Idlerx uses.
pub const MIN_VERSION: u32 = 140;
/// A network's Firefox with no account left closes after this long.
const IDLE_CLOSE: Duration = Duration::from_secs(20);
/// How long an account's window may take to show up.
const OPEN_TIMEOUT: Duration = Duration::from_secs(40);
/// How long an extension that restarted (Firefox still running) may take to
/// connect again.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a network's Firefox may take to connect its extension.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Keys pressed inside a page that belong to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    /// F11: focus mode.
    Focus,
    /// Esc: leaves focus mode (the page still gets it).
    Escape,
    /// Ctrl+1…9: shows the account at that place of the list.
    Account(u8),
}

/// Something went wrong with a page (see `Page::failure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The account's Firefox (or its window) is gone. (A page whose
    /// process crashed shows Firefox's own "restore" page.)
    BrowserGone,
}

/// A login the user just made in an account (Idlerx offers to save it).
#[derive(Clone, Debug, PartialEq)]
pub struct LoginOffer {
    pub account: String,
    pub origin: String,
    pub username: String,
    pub password: String,
}

/// State of an account's page, kept up to date by the extension's events.
pub struct Page {
    pub id: String,
    /// Container name (the account's profile).
    pub profile: String,
    pub net: String,
    pub window: Cell<Option<Window>>,
    marker: RefCell<Option<String>>,
    pub url: RefCell<String>,
    pub title: RefCell<String>,
    pub audible: Cell<bool>,
    pub zoom: Cell<f64>,
    pub loaded: Cell<bool>,
    pub failure: Cell<Option<Failure>>,
    pub got_focus: Cell<bool>,
    /// Last click or key a macro sent, or when the window was embedded (the
    /// focus that follows is not the user's).
    pub macro_input: Cell<Option<Instant>>,
    pub hotkey: Cell<Option<Hotkey>>,
    pub recording: Cell<bool>,
    pub recorded: RefCell<Vec<Value>>,
    /// Hidden state last told to the page, and "keep running".
    pub hidden: Cell<(bool, bool)>,
    pub private: bool,
}

impl Page {
    /// The page got the mouse or keyboard: the user's, unless a macro just
    /// sent input to it.
    fn focused(&self) {
        let by_macro = self
            .macro_input
            .get()
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
        if !by_macro {
            self.got_focus.set(true);
        }
    }
}

/// What an account opens with.
pub struct OpenSpec {
    pub id: String,
    pub profile: String,
    pub url: String,
    pub private: bool,
    pub zoom: f64,
    pub muted: bool,
    pub net: String,
    pub options: profile::Options,
}

enum Incoming {
    Connected {
        net: String,
    },
    Message {
        net: String,
        msg: Value,
    },
    Disconnected {
        net: String,
    },
    Started {
        net: String,
        generation: u64,
        result: Result<u32, String>,
    },
}

type Writers = Arc<Mutex<HashMap<String, (u64, TcpStream)>>>;

struct Net {
    options: profile::Options,
    generation: u64,
    /// Firefox's launcher process (it waits for the browser), and the
    /// browser itself once found.
    launcher: Option<u32>,
    browser: Option<u32>,
    connected: bool,
    /// Commands waiting for the extension to connect.
    pending: Vec<Value>,
    /// No account left since (it closes after IDLE_CLOSE).
    idle_since: Option<Instant>,
    /// The start window, hidden (see `Window::make_keeper`).
    keeper: Option<Window>,
    started: Instant,
    closing: bool,
    /// The link to the extension dropped with Firefox still running (its
    /// extension restarting): given up at this moment if it does not come
    /// back.
    reconnect_until: Option<Instant>,
}

type Ready = Box<dyn FnOnce(Result<Rc<Page>, String>)>;
/// Gets a page's picture (PNG) once it is taken.
type Capture = Box<dyn FnOnce(Option<Vec<u8>>)>;

struct Waiting {
    page: Rc<Page>,
    ready: Ready,
    since: Instant,
}

pub struct Engine {
    port: u16,
    token: String,
    rx: mpsc::Receiver<Incoming>,
    tx: mpsc::Sender<Incoming>,
    writers: Writers,
    waker: Waker,
    firefox: Option<PathBuf>,
    job: isize,
    nets: HashMap<String, Net>,
    pages: HashMap<String, Rc<Page>>,
    waiting: Vec<Waiting>,
    parent: isize,
    /// Profiles of every account (containers of the others are removed).
    keep: Vec<String>,
    scheme: &'static str,
    save_passwords: bool,
    next_req: u64,
    captures: HashMap<u64, Capture>,
    offers: Vec<LoginOffer>,
    generation: u64,
    started_at: std::time::SystemTime,
    /// Windows held back while their Firefox opened accounts (see
    /// `window::guard`): accounts' are embedded, the start window becomes
    /// the keeper, the others (opened by pages) are shown again.
    held: Vec<(Window, Instant)>,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Runs `f` with the engine (None before `init`).
pub fn with<R>(f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    ENGINE.with(|e| e.borrow_mut().as_mut().map(f))
}

/// Work to do once the engine is free again (it may call the engine).
type Later = Box<dyn FnOnce()>;

/// The engine's work of every frame (see `Engine::pump`), then the
/// callbacks it produced.
pub fn pump() -> Vec<LoginOffer> {
    let Some((later, offers)) = with(|e| (e.pump(), e.take_offers())) else {
        return Vec::new();
    };
    for f in later {
        f();
    }
    offers
}

/// Starts the engine (the local socket for the extension and the native
/// app registration). `waker` asks the interface for a frame.
pub fn init(waker: Waker) -> Result<(), String> {
    let firefox = find_firefox().map(|(p, _)| p);
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = crate::config::new_id() + &crate::config::new_id();
    let (tx, rx) = mpsc::channel();
    let writers: Writers = Arc::default();
    accept_loop(
        listener,
        token.clone(),
        tx.clone(),
        writers.clone(),
        waker.clone(),
    );
    let _ = host::register();
    let w = waker.clone();
    window::watch_focus(move || w());
    let engine = Engine {
        port,
        token,
        rx,
        tx,
        writers,
        waker,
        firefox,
        job: job_object(),
        nets: HashMap::new(),
        pages: HashMap::new(),
        waiting: Vec::new(),
        parent: 0,
        keep: Vec::new(),
        scheme: "auto",
        save_passwords: true,
        next_req: 1,
        captures: HashMap::new(),
        offers: Vec::new(),
        generation: 0,
        started_at: std::time::SystemTime::now(),
        held: Vec::new(),
    };
    ENGINE.with(|e| *e.borrow_mut() = Some(engine));
    Ok(())
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    tx: mpsc::Sender<Incoming>,
    writers: Writers,
    waker: Waker,
) {
    std::thread::spawn(move || {
        for (conn, stream) in (1u64..).zip(listener.incoming().flatten()) {
            let (tx, writers, waker, token) =
                (tx.clone(), writers.clone(), waker.clone(), token.clone());
            std::thread::spawn(move || {
                let mut reader = stream;
                let _ = reader.set_nodelay(true);
                // the relay's first message: the key and the network
                let Some(hello) = host::read_frame(&mut reader) else {
                    return;
                };
                let Ok(hello) = serde_json::from_slice::<Value>(&hello) else {
                    return;
                };
                if hello["token"].as_str() != Some(token.as_str()) {
                    return;
                }
                let net = hello["net"].as_str().unwrap_or_default().to_string();
                let Ok(writer) = reader.try_clone() else {
                    return;
                };
                writers.lock().unwrap().insert(net.clone(), (conn, writer));
                let _ = tx.send(Incoming::Connected { net: net.clone() });
                waker();
                while let Some(frame) = host::read_frame(&mut reader) {
                    if let Ok(msg) = serde_json::from_slice::<Value>(&frame) {
                        let _ = tx.send(Incoming::Message {
                            net: net.clone(),
                            msg,
                        });
                        waker();
                    }
                }
                {
                    let mut w = writers.lock().unwrap();
                    if w.get(&net).is_some_and(|(c, _)| *c == conn) {
                        w.remove(&net);
                    }
                }
                let _ = tx.send(Incoming::Disconnected { net });
                waker();
            });
        }
    });
}

impl Engine {
    pub fn set_parent(&mut self, hwnd: isize) {
        self.parent = hwnd;
    }

    pub fn set_keep(&mut self, profiles: Vec<String>) {
        if profiles != self.keep {
            self.keep = profiles;
            let nets: Vec<String> = self.nets.keys().cloned().collect();
            for n in nets {
                let msg = json!({ "cmd": "sync", "keep": self.keep });
                self.send(&n, msg);
            }
        }
    }

    pub fn set_scheme(&mut self, scheme: &'static str) {
        if scheme != self.scheme {
            self.scheme = scheme;
            let nets: Vec<String> = self.nets.keys().cloned().collect();
            for n in nets {
                self.send(&n, json!({ "cmd": "scheme", "value": scheme }));
            }
        }
    }

    pub fn set_save_passwords(&mut self, on: bool) {
        self.save_passwords = on;
    }

    /// uBlock Origin's dashboard (settings and lists), opened by the main
    /// network's Firefox (started for it when needed).
    pub fn open_blocker_dashboard(&mut self) {
        let net = self
            .nets
            .keys()
            .find(|n| n.as_str() == "main")
            .or_else(|| self.nets.keys().next())
            .cloned();
        if let Some(net) = net {
            self.send(&net, json!({ "cmd": "dashboard" }));
        }
    }

    pub fn take_offers(&mut self) -> Vec<LoginOffer> {
        std::mem::take(&mut self.offers)
    }

    /// Sends a command to a network's extension (kept until it connects).
    pub fn send(&mut self, net: &str, msg: Value) {
        let Some(n) = self.nets.get_mut(net) else {
            return;
        };
        if !n.connected {
            n.pending.push(msg);
            return;
        }
        let bytes = host::frame(msg.to_string().as_bytes());
        let mut w = self.writers.lock().unwrap();
        if let Some((_, s)) = w.get_mut(net) {
            let _ = s.write_all(&bytes);
        }
    }

    /// Opens an account; `ready` gets its page once the window is in place.
    pub fn open(&mut self, spec: OpenSpec, ready: Ready) {
        let Some(firefox) = self.firefox.clone() else {
            ready(Err("Firefox não encontrado".into()));
            return;
        };
        // a running Firefox with other settings (a VPN gate that changed):
        // started again, its accounts reopen
        let differs = |a: &profile::Options, b: &profile::Options| {
            (a.economy, a.fps, a.proxy) != (b.economy, b.fps, b.proxy)
        };
        if self
            .nets
            .get(&spec.net)
            .is_some_and(|n| differs(&n.options, &spec.options))
        {
            self.stop_net(&spec.net);
        }
        if !self.nets.contains_key(&spec.net) {
            self.start_net(&spec.net, spec.options.clone(), firefox);
        }
        if let Some(old) = self.pages.remove(&spec.id) {
            old.window.get().inspect(|w| w.close());
        }
        let page = Rc::new(Page {
            id: spec.id.clone(),
            profile: spec.profile.clone(),
            net: spec.net.clone(),
            window: Cell::new(None),
            marker: RefCell::new(None),
            url: RefCell::new(spec.url.clone()),
            title: RefCell::new(String::new()),
            audible: Cell::new(false),
            zoom: Cell::new(spec.zoom),
            loaded: Cell::new(false),
            failure: Cell::new(None),
            got_focus: Cell::new(false),
            macro_input: Cell::new(None),
            hotkey: Cell::new(None),
            recording: Cell::new(false),
            recorded: RefCell::new(Vec::new()),
            hidden: Cell::new((true, true)),
            private: spec.private,
        });
        self.pages.insert(spec.id.clone(), page.clone());
        if let Some(n) = self.nets.get_mut(&spec.net) {
            n.idle_since = None;
        }
        self.send(
            &spec.net,
            json!({
                "cmd": "open",
                "id": spec.id,
                "container": spec.profile,
                "url": spec.url,
                "private": spec.private,
                "zoom": spec.zoom,
                "muted": spec.muted,
            }),
        );
        window::guard(self.job);
        self.waiting.push(Waiting {
            page,
            ready,
            since: Instant::now(),
        });
    }

    /// The account's page was closed by Idlerx.
    pub fn close(&mut self, id: &str) {
        self.waiting.retain(|w| w.page.id != id);
        let Some(page) = self.pages.remove(id) else {
            return;
        };
        if let Some(w) = page.window.get() {
            w.hide();
        }
        let net = page.net.clone();
        self.send(&net, json!({ "cmd": "close", "id": id }));
        if let Some(w) = page.window.get() {
            // the extension closes it; a window still there later goes anyway
            let _ = w;
        }
        if !self.pages.values().any(|p| p.net == net) {
            if let Some(n) = self.nets.get_mut(&net) {
                n.idle_since = Some(Instant::now());
            }
        }
    }

    pub fn capture(&mut self, page: &Page, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        let req = self.next_req;
        self.next_req += 1;
        self.captures.insert(req, Box::new(done));
        let net = page.net.clone();
        self.send(&net, json!({ "cmd": "capture", "id": page.id, "req": req }));
    }

    fn start_net(&mut self, net: &str, options: profile::Options, firefox: PathBuf) {
        self.generation += 1;
        let generation = self.generation;
        let mut options = options;
        // development: the extension goes in through the debugger
        if !profile::signed_extension() {
            options.debugger = free_port();
        }
        self.nets.insert(
            net.to_string(),
            Net {
                options: options.clone(),
                generation,
                launcher: None,
                browser: None,
                connected: false,
                pending: vec![json!({ "cmd": "scheme", "value": self.scheme })],
                idle_since: None,
                keeper: None,
                started: Instant::now(),
                closing: false,
                reconnect_until: None,
            },
        );
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        window::guard(self.job);
        let (port, token, job, net) = (self.port, self.token.clone(), self.job, net.to_string());
        std::thread::spawn(move || {
            let result = launch(&firefox, &net, &options, port, &token, job);
            let _ = tx.send(Incoming::Started {
                net,
                generation,
                result,
            });
            waker();
        });
    }

    fn stop_net(&mut self, net: &str) {
        let Some(n) = self.nets.remove(net) else {
            return;
        };
        if let Some(pid) = n.browser {
            for w in window::windows_of(pid, false) {
                w.close();
            }
            kill(pid);
        }
        if let Some(pid) = n.launcher {
            kill(pid);
        }
        self.writers.lock().unwrap().remove(net);
        for p in self.pages.values().filter(|p| p.net == net) {
            p.failure.set(Some(Failure::BrowserGone));
        }
    }

    /// Closes every Firefox (Idlerx is closing): each closes its windows
    /// normally; the job ends whatever is left when Idlerx exits.
    pub fn shutdown(&mut self) {
        let nets: Vec<String> = self.nets.keys().cloned().collect();
        for n in nets {
            if let Some(pid) = self.nets[&n].browser {
                for w in window::windows_of(pid, false) {
                    w.close();
                }
            }
        }
        for p in self.pages.values() {
            if let Some(w) = p.window.get() {
                w.close();
            }
        }
    }

    /// Work of every frame: events from the extensions, windows to embed,
    /// Firefoxes to close. Asks for frames while something is on its way.
    fn pump(&mut self) -> Vec<Later> {
        let mut later: Vec<Later> = Vec::new();
        // the user clicked in an account's window
        for w in window::take_focused() {
            let page = self.pages.values().find(|p| {
                p.window
                    .get()
                    .is_some_and(|pw| pw.visible() && w.within(pw))
            });
            if let Some(p) = page {
                p.focused();
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Incoming::Connected { net } => self.connected(&net),
                Incoming::Message { net, msg } => {
                    if let Some(f) = self.event(&net, msg) {
                        later.push(f);
                    }
                }
                Incoming::Disconnected { net } => {
                    let Some(n) = self.nets.get(&net).filter(|n| n.connected) else {
                        continue;
                    };
                    let closing = n.closing;
                    let alive = n.browser.is_some_and(|pid| exit_code(pid).is_none());
                    if alive && !closing {
                        // the extension restarted (an update, for one):
                        // Firefox and its windows are fine, it connects again
                        log(&net, "a ligação com a extensão caiu; esperando ela voltar (o Firefox continua aberto)");
                        if let Some(n) = self.nets.get_mut(&net) {
                            n.connected = false;
                            n.reconnect_until = Some(Instant::now() + RECONNECT_TIMEOUT);
                        }
                        continue;
                    }
                    if closing {
                        log(&net, "Firefox fechado (sem contas)");
                    } else {
                        let how = match n.browser.map(exit_code) {
                            Some(Some(code)) => format!("o Firefox fechou (código {code:#x})"),
                            _ => "o Firefox fechou".to_string(),
                        };
                        match new_crash_report(self.started_at) {
                            Some(d) => log(&net, &format!("{how}; relatório de travamento: {d}")),
                            None => log(&net, &how),
                        }
                    }
                    self.net_gone(&net, closing);
                    later.extend(self.fail_waiting(&net, "o Firefox fechou"));
                }
                Incoming::Started {
                    net,
                    generation,
                    result,
                } => {
                    let Some(n) = self
                        .nets
                        .get_mut(&net)
                        .filter(|n| n.generation == generation)
                    else {
                        continue;
                    };
                    match result {
                        Ok(pid) => {
                            n.launcher = Some(pid);
                            log(&net, "Firefox iniciado");
                        }
                        Err(e) => {
                            log(&net, &format!("o Firefox não iniciou: {e}"));
                            self.nets.remove(&net);
                            later.extend(self.fail_waiting(&net, &e));
                        }
                    }
                }
            }
        }
        self.guard_windows();
        // held windows are looked at again until sorted out
        let mut busy = !self.held.is_empty();
        // the window Firefox opens when it starts: hidden, it keeps that
        // Firefox running while it has no account
        for n in self.nets.values_mut() {
            if n.browser.is_none() {
                if let Some(l) = n.launcher {
                    n.browser = child_process(l);
                }
            }
            if let (Some(pid), None) = (n.browser, n.keeper) {
                if let Some(w) = window::windows_of(pid, true).into_iter().next() {
                    w.make_keeper();
                    n.keeper = Some(w);
                }
            }
            if !n.connected {
                busy = true;
            }
        }
        // account windows to put in place
        let parent = self.parent;
        let mut done = Vec::new();
        for (i, w) in self.waiting.iter().enumerate() {
            let marker = w.page.marker.borrow().clone();
            if let Some(m) = marker {
                if let Some(win) = window::find(&m, parent) {
                    // embedding moves the focus for a moment: not the user's
                    w.page.macro_input.set(Some(Instant::now()));
                    win.embed(parent);
                    w.page.window.set(Some(win));
                    done.push((i, Ok(())));
                    continue;
                }
            }
            if w.since.elapsed() > OPEN_TIMEOUT {
                done.push((i, Err("a janela da conta não apareceu".to_string())));
            } else {
                busy = true;
            }
        }
        for (i, r) in done.into_iter().rev() {
            let w = self.waiting.remove(i);
            match r {
                Ok(()) => later.push(Box::new(move || (w.ready)(Ok(w.page)))),
                Err(e) => {
                    self.pages.remove(&w.page.id);
                    later.push(Box::new(move || (w.ready)(Err(e))));
                }
            }
        }
        // an extension that restarted and did not connect again in time
        let lost: Vec<String> = self
            .nets
            .iter()
            .filter(|(_, n)| !n.connected && n.reconnect_until.is_some_and(|t| Instant::now() > t))
            .map(|(k, _)| k.clone())
            .collect();
        for net in lost {
            log(&net, "a extensão não voltou em 15 s: fechando esse Firefox");
            self.net_gone(&net, false);
            later.extend(self.fail_waiting(&net, "a extensão do Firefox não voltou"));
        }
        // a Firefox that never connected: given up (its accounts can be
        // opened again)
        let stuck: Vec<String> = self
            .nets
            .iter()
            .filter(|(_, n)| {
                !n.connected && n.reconnect_until.is_none() && n.started.elapsed() > CONNECT_TIMEOUT
            })
            .map(|(k, _)| k.clone())
            .collect();
        for net in stuck {
            log(&net, "o Firefox não conectou a extensão em 30 s");
            if let Some(n) = self.nets.remove(&net) {
                for pid in [n.browser, n.launcher].into_iter().flatten() {
                    kill(pid);
                }
            }
            later.extend(self.fail_waiting(&net, "o Firefox não respondeu"));
        }
        // a Firefox without accounts for a while closes (frees its memory)
        let idle: Vec<String> = self
            .nets
            .iter()
            .filter(|(_, n)| n.idle_since.is_some_and(|t| t.elapsed() > IDLE_CLOSE) && !n.closing)
            .map(|(k, _)| k.clone())
            .collect();
        for net in idle {
            if self.pages.values().any(|p| p.net == net) {
                continue;
            }
            if let Some(n) = self.nets.get_mut(&net) {
                n.closing = true;
                if let Some(pid) = n.browser {
                    for w in window::windows_of(pid, false) {
                        w.close();
                    }
                }
            }
        }
        if busy {
            let waker = self.waker.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                waker();
            });
        }
        later
    }

    /// New windows of a Firefox that starts or opens accounts are held back
    /// (hidden as they show); held ones are sorted out here.
    fn guard_windows(&mut self) {
        for n in self.nets.values_mut().filter(|n| n.browser.is_none()) {
            n.browser = n.launcher.and_then(child_process);
        }
        let opening = !self.waiting.is_empty() || self.nets.values().any(|n| !n.connected);
        window::guard(if opening { self.job } else { 0 });
        self.held.extend(window::take_held());
        let keepers: Vec<Window> = self.nets.values().filter_map(|n| n.keeper).collect();
        let mut keep = Vec::new();
        for (w, since) in std::mem::take(&mut self.held) {
            if !w.exists_top_level() || keepers.contains(&w) || w.is_account() {
                continue;
            }
            // the first window of a starting Firefox: its start window
            let net = self
                .nets
                .values_mut()
                .find(|n| n.keeper.is_none() && n.browser.is_some_and(|b| b == w.pid()));
            if let Some(n) = net {
                w.make_keeper();
                n.keeper = Some(w);
                continue;
            }
            // not an account's after a moment (its Firefox done opening
            // accounts): a window a page opened
            let opening = self.nets.iter().any(|(k, n)| {
                n.browser.is_some_and(|b| b == w.pid())
                    && (!n.connected || self.waiting.iter().any(|x| &x.page.net == k))
            });
            let wait = if opening { 15 } else { 3 };
            if since.elapsed() > Duration::from_secs(wait) {
                w.release();
            } else {
                keep.push((w, since));
            }
        }
        self.held = keep;
    }

    /// A network's Firefox is gone (or given up): whatever is left of it
    /// ends (a new one needs its profile) and its accounts open again.
    fn net_gone(&mut self, net: &str, closing: bool) {
        if let Some(n) = self.nets.remove(net) {
            for pid in [n.browser, n.launcher].into_iter().flatten() {
                kill(pid);
            }
        }
        self.writers.lock().unwrap().remove(net);
        if !closing {
            for p in self.pages.values().filter(|p| p.net == net) {
                p.failure.set(Some(Failure::BrowserGone));
            }
        }
    }

    fn fail_waiting(&mut self, net: &str, why: &str) -> Vec<Later> {
        let (fail, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|w| w.page.net == net);
        self.waiting = keep;
        fail.into_iter()
            .map(|w| {
                self.pages.remove(&w.page.id);
                let why = why.to_string();
                Box::new(move || (w.ready)(Err(why))) as Later
            })
            .collect()
    }

    fn connected(&mut self, net: &str) {
        let Some(n) = self.nets.get_mut(net) else {
            return;
        };
        n.connected = true;
        let again = n.reconnect_until.take().is_some();
        if again {
            log(net, "a extensão voltou");
        } else {
            log(
                net,
                &format!(
                    "extensão conectada ({:.1} s)",
                    n.started.elapsed().as_secs_f32()
                ),
            );
        }
        let mut pending = std::mem::take(&mut n.pending);
        if again {
            // its accounts, which it no longer knows (their windows are
            // still open), with their state
            let accounts: Vec<Value> = self
                .pages
                .values()
                .filter(|p| p.net == net)
                .map(|p| {
                    let (hidden, awake) = p.hidden.get();
                    json!({
                        "id": p.id,
                        "container": p.profile,
                        "private": p.private,
                        "hidden": hidden,
                        "awake": awake,
                        "rec": p.recording.get(),
                    })
                })
                .collect();
            pending.insert(0, json!({ "cmd": "adopt", "accounts": accounts }));
        }
        // first of all, the containers that must go (never with an empty
        // list: that would be every account's)
        if !self.keep.is_empty() {
            pending.insert(0, json!({ "cmd": "sync", "keep": self.keep }));
        }
        for m in pending {
            self.send(net, m);
        }
    }

    fn page(&self, msg: &Value) -> Option<Rc<Page>> {
        msg["id"]
            .as_str()
            .and_then(|id| self.pages.get(id).cloned())
    }

    fn event(&mut self, net: &str, msg: Value) -> Option<Later> {
        let page = self.page(&msg);
        match msg["ev"].as_str().unwrap_or_default() {
            "hello" => {
                // a new connection of a running Firefox (its extension was
                // reloaded): nothing to do, commands flow again
            }
            "opened" => {
                if let Some(p) = page {
                    *p.marker.borrow_mut() = msg["marker"].as_str().map(String::from);
                }
            }
            "page" => {
                if let Some(p) = page {
                    if let Some(u) = msg["url"].as_str() {
                        *p.url.borrow_mut() = u.to_string();
                    }
                    if let Some(t) = msg["title"].as_str() {
                        *p.title.borrow_mut() = t.to_string();
                    }
                    p.audible.set(msg["audible"].as_bool().unwrap_or(false));
                }
            }
            "loaded" => {
                if let Some(p) = page {
                    p.loaded.set(true);
                    if p.recording.get() {
                        // a new document starts without recording
                        self.send(net, json!({ "cmd": "record", "id": p.id, "on": true }));
                    }
                }
            }
            "zoom" => {
                if let (Some(p), Some(z)) = (page, msg["zoom"].as_f64()) {
                    p.zoom.set(z);
                }
            }
            "closed" => {
                if let Some(p) = page {
                    p.failure.set(Some(Failure::BrowserGone));
                }
            }
            "rec" => {
                if let Some(p) = page.filter(|p| p.recording.get()) {
                    p.recorded.borrow_mut().push(msg["data"].clone());
                }
            }
            "key" => {
                if let Some(p) = page {
                    p.hotkey.set(match msg["key"].as_str() {
                        Some("F11") => Some(Hotkey::Focus),
                        Some("Escape") => Some(Hotkey::Escape),
                        Some(k) => k
                            .strip_prefix("Ctrl+")
                            .and_then(|n| n.parse().ok())
                            .map(Hotkey::Account),
                        None => None,
                    });
                }
            }
            "focus" => {
                if let Some(p) = page {
                    p.focused();
                }
            }
            "captured" => {
                if let Some(done) = msg["req"].as_u64().and_then(|r| self.captures.remove(&r)) {
                    use base64::Engine as _;
                    let png = msg["data"]
                        .as_str()
                        .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok());
                    return Some(Box::new(move || done(png)));
                }
            }
            "logins" => {
                let list = match (&page, msg["origin"].as_str()) {
                    (Some(p), Some(origin)) if self.save_passwords => {
                        crate::passwords::logins(&p.profile, origin)
                    }
                    _ => Vec::new(),
                };
                let list: Vec<Value> = list
                    .into_iter()
                    .map(|l| json!({ "username": l.username, "password": l.password }))
                    .collect();
                self.send(
                    net,
                    json!({ "cmd": "logins", "req": msg["req"], "list": list }),
                );
            }
            "login" => {
                if let (Some(p), true) = (page, self.save_passwords) {
                    let offer = LoginOffer {
                        account: p.id.clone(),
                        origin: msg["origin"].as_str().unwrap_or_default().to_string(),
                        username: msg["username"].as_str().unwrap_or_default().to_string(),
                        password: msg["password"].as_str().unwrap_or_default().to_string(),
                    };
                    if !offer.password.is_empty()
                        && !crate::passwords::is_saved(&p.profile, &offer)
                        && !crate::passwords::never(&p.profile, &offer.origin)
                    {
                        self.offers.retain(|o| o.account != offer.account);
                        self.offers.push(offer);
                    }
                }
            }
            "error" | "log" => log(net, msg["what"].as_str().unwrap_or_default()),
            _ => {}
        }
        None
    }
}

/// Starts a network's Firefox (in the job, so it never outlives Idlerx).
fn launch(
    firefox: &std::path::Path,
    net: &str,
    o: &profile::Options,
    port: u16,
    token: &str,
    job: isize,
) -> Result<u32, String> {
    let dir = profile::prepare(net, o)?;
    // a Firefox left with this profile (still closing) would keep it
    // locked: the new one would quit at once
    end_firefox_using(&dir);
    let quote = |a: &str| format!("\"{}\"", a.replace('"', ""));
    let mut line = vec![
        quote(&firefox.to_string_lossy()),
        "--new-instance".into(),
        "-no-remote".into(),
        "--wait-for-browser".into(),
        "-profile".into(),
        quote(&dir.to_string_lossy()),
    ];
    if let Some(p) = o.debugger {
        line.push("--start-debugger-server".into());
        line.push(p.to_string());
    }
    line.push("about:blank".into());
    let extra = [
        (host::ENV_PORT, port.to_string()),
        (host::ENV_TOKEN, token.to_string()),
        (host::ENV_NET, net.to_string()),
        // a crash leaves a report in Idlerx's folder (no window, nothing sent)
        ("MOZ_CRASHREPORTER_NO_REPORT", "1".to_string()),
        (
            "MOZ_CRASHREPORTER_DATA_DIRECTORY",
            crash_dir().to_string_lossy().to_string(),
        ),
    ];
    let pid = spawn_minimized(&line.join(" "), &extra, job)?;
    if let Some(p) = o.debugger {
        rdp::install_extension(p)?;
    }
    Ok(pid)
}

/// Starts a process (suspended until it is in the job) with its first
/// window minimized and without taking the focus: Firefox's start window
/// never shows on the screen. Returns its id.
fn spawn_minimized(
    command_line: &str,
    extra_env: &[(&str, String)],
    job: isize,
) -> Result<u32, String> {
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
    // the environment: Idlerx's own, plus the variables for the relay
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| !extra_env.iter().any(|(e, _)| e.eq_ignore_ascii_case(k)))
        .collect();
    env.extend(extra_env.iter().map(|(k, v)| (k.to_string(), v.clone())));
    env.sort_by_key(|(k, _)| k.to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in &env {
        block.extend(format!("{k}={v}").encode_utf16());
        block.push(0);
    }
    block.push(0);
    let mut line: Vec<u16> = command_line.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_SHOWMINNOACTIVE as u16;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            block.as_ptr() as *const _,
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err(format!(
                "CreateProcess: {}",
                windows_sys::Win32::Foundation::GetLastError()
            ));
        }
        assign_to_job(job, pi.hProcess as isize);
        ResumeThread(pi.hThread);
        windows_sys::Win32::Foundation::CloseHandle(pi.hThread);
        windows_sys::Win32::Foundation::CloseHandle(pi.hProcess);
        Ok(pi.dwProcessId)
    }
}

/// A line in the pages log about a network's Firefox.
fn log(net: &str, what: &str) {
    crate::events::log(&format!("Firefox {net}"), what);
}

/// Ends the Firefox processes started with `profile` (only Idlerx's own
/// profiles are ever passed here).
fn end_firefox_using(profile: &std::path::Path) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let norm = |s: &str| s.replace('/', "\\").to_ascii_lowercase();
    let needle = norm(&profile.to_string_lossy());
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    let mut ended = false;
    for (pid, p) in sys.processes() {
        let is_firefox = p
            .name()
            .to_string_lossy()
            .eq_ignore_ascii_case("firefox.exe");
        let ours = p
            .cmd()
            .iter()
            .any(|a| norm(&a.to_string_lossy()).contains(&needle));
        if is_firefox && ours {
            kill(pid.as_u32());
            ended = true;
        }
    }
    if ended {
        // the profile's lock goes with the process
        std::thread::sleep(Duration::from_millis(800));
    }
}

fn free_port() -> Option<u16> {
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

// ------------------------------------------------------------- processes --

fn job_object() -> isize {
    use windows_sys::Win32::System::JobObjects::*;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as isize
    }
}

fn assign_to_job(job: isize, process: isize) {
    if job != 0 {
        unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(job as _, process as _)
        };
    }
}

/// A child process of `parent` named firefox.exe (the browser started by
/// Firefox's launcher).
fn child_process(parent: u32) -> Option<u32> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return None;
        }
        let mut p: PROCESSENTRY32W = std::mem::zeroed();
        p.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut p);
        while ok != 0 {
            let name = String::from_utf16_lossy(&p.szExeFile)
                .trim_end_matches('\0')
                .to_ascii_lowercase();
            if p.th32ParentProcessID == parent && name == "firefox.exe" {
                found = Some(p.th32ProcessID);
                break;
            }
            ok = Process32NextW(snap, &mut p);
        }
        windows_sys::Win32::Foundation::CloseHandle(snap);
        found
    }
}

/// Exit code of a process; None while it runs.
fn exit_code(pid: u32) -> Option<u32> {
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return Some(0);
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        windows_sys::Win32::Foundation::CloseHandle(h);
        (ok != 0 && code != STILL_ACTIVE).then_some(code)
    }
}

/// Where Idlerx's Firefox leaves its crash reports.
fn crash_dir() -> PathBuf {
    profile::root().join("crashes")
}

/// A crash report written since `since` (its file name).
fn new_crash_report(since: std::time::SystemTime) -> Option<String> {
    let mut newest = None;
    let mut stack = vec![crash_dir()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "dmp") {
                let when = e.metadata().and_then(|m| m.modified()).ok();
                if when.is_some_and(|w| w >= since) {
                    newest = Some(path.to_string_lossy().to_string());
                }
            }
        }
    }
    newest
}

fn kill(pid: u32) {
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
    }
}

// --------------------------------------------------------------- install --

/// The installed Firefox (registry of the official installer): its
/// firefox.exe and version.
pub fn find_firefox() -> Option<(PathBuf, String)> {
    use windows_sys::Win32::System::Registry::*;
    let read = |root: HKEY, key: &str, value: Option<&str>| -> Option<String> {
        let k = crate::single::wide(key);
        let v = value.map(crate::single::wide);
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                k.as_ptr(),
                v.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut len,
            )
        };
        (r == 0).then(|| {
            let n = (len as usize / 2).saturating_sub(1);
            String::from_utf16_lossy(&buf[..n])
        })
    };
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Some(current) = read(
            root,
            r"SOFTWARE\Mozilla\Mozilla Firefox",
            Some("CurrentVersion"),
        ) else {
            continue;
        };
        let exe = read(
            root,
            &format!(r"SOFTWARE\Mozilla\Mozilla Firefox\{current}\Main"),
            Some("PathToExe"),
        )
        .map(PathBuf::from)
        .filter(|p| p.exists());
        if let Some(exe) = exe {
            let version = current
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string();
            return Some((exe, version));
        }
    }
    let default = PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe");
    default.exists().then(|| (default, String::new()))
}

/// The Firefox version, or why Idlerx cannot use it.
pub fn runtime_version() -> Result<String, String> {
    let Some((_, version)) = find_firefox() else {
        return Err(tr!("Firefox não está instalado.", "Firefox is not installed.").into());
    };
    let major: u32 = version
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(MIN_VERSION);
    if major < MIN_VERSION {
        return Err(trf!(
            "Firefox {version} é antigo demais: o Idlerx precisa do {MIN_VERSION} ou mais novo.",
            "Firefox {version} is too old: Idlerx needs {MIN_VERSION} or newer."
        ));
    }
    Ok(version)
}

/// Official installer (latest Firefox, in the interface's language).
pub fn installer_url() -> String {
    let lang = if crate::i18n::pt() { "pt-BR" } else { "en-US" };
    format!("https://download.mozilla.org/?product=firefox-latest-ssl&os=win64&lang={lang}")
}

/// Downloads Mozilla's installer, checks its signature and runs it (it
/// asks Windows for permission). Blocking: call from a thread.
pub fn install() -> Result<(), String> {
    let bytes = crate::update::fetch(&installer_url(), 200 << 20)?;
    let path = std::env::temp_dir().join("idlerx-firefox-setup.exe");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    match crate::update::signer(&path) {
        Some(s) if s.contains("Mozilla Corporation") => {}
        other => {
            let _ = std::fs::remove_file(&path);
            return Err(trf!(
                "O instalador baixado não tem a assinatura da Mozilla ({:?}).",
                "The downloaded installer is not signed by Mozilla ({:?}).",
                other
            ));
        }
    }
    let status = std::process::Command::new(&path)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if find_firefox().is_some() {
        Ok(())
    } else {
        Err(trf!(
            "A instalação do Firefox não terminou ({status}).",
            "Firefox's installation did not finish ({status})."
        ))
    }
}
