//! System tray: the X sends the window here (the games keep running), the
//! icon gets a red dot while a group's VPN is down, and Windows
//! notifications tell about drops.
//!
//! "In the tray" is the window minimized without its taskbar button: a
//! minimized window keeps the app's bookkeeping running (VPN of groups,
//! accounts waiting to open, metrics), which a hidden one would not.

use std::sync::mpsc::{channel, Receiver};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What the tray asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Show,
    OpenAll,
    PauseAll,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    normal: Icon,
    alert: Icon,
    alert_on: bool,
    menu: Menu,
    /// Disabled lines at the top of the menu (VPN of each group), followed
    /// by a separator when there are any.
    status: Vec<MenuItem>,
    separator: PredefinedMenuItem,
    status_texts: Vec<String>,
    tooltip: String,
    rx: Receiver<Command>,
}

/// Icon pixels with a red dot (a group's VPN is down).
fn with_red_dot(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let s = size as f32;
    let (cx, cy, r) = (s * 0.74, s * 0.74, s * 0.22);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let color = if d <= r - s * 0.05 {
                [0xef, 0x44, 0x44, 0xff]
            } else if d <= r {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                continue;
            };
            let i = ((y * size + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&color);
        }
    }
    out
}

impl Tray {
    /// Creates the icon (on the UI thread). `wake` repaints the interface
    /// so the commands are picked up.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Option<Tray> {
        let size = 64;
        let normal = Icon::from_rgba(crate::ICON_RGBA.to_vec(), size, size).ok()?;
        let alert = Icon::from_rgba(with_red_dot(crate::ICON_RGBA, size), size, size).ok()?;

        let show = MenuItem::new(tr!("Mostrar o Idlerx", "Show Idlerx"), true, None);
        let open_all = MenuItem::new(
            tr!("Abrir todas as contas", "Open all accounts"),
            true,
            None,
        );
        let pause_all = MenuItem::new(
            tr!("Pausar todas as contas", "Pause all accounts"),
            true,
            None,
        );
        let quit = MenuItem::new(tr!("Sair", "Quit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &show,
            &PredefinedMenuItem::separator(),
            &open_all,
            &pause_all,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .ok()?;

        let (tx, rx) = channel();
        let ids = [
            (show.id().clone(), Command::Show),
            (open_all.id().clone(), Command::OpenAll),
            (pause_all.id().clone(), Command::PauseAll),
            (quit.id().clone(), Command::Quit),
        ];
        let wake = std::sync::Arc::new(wake);
        let (tx2, wake2) = (tx.clone(), wake.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, c)) = ids.iter().find(|(id, _)| *id == e.id) {
                let _ = tx2.send(*c);
                wake2();
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = tx.send(Command::Show);
                wake();
            }
        }));

        let icon = TrayIconBuilder::new()
            .with_tooltip("Idlerx")
            .with_icon(normal.clone())
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        Some(Tray {
            icon,
            normal,
            alert,
            alert_on: false,
            menu,
            status: Vec::new(),
            separator: PredefinedMenuItem::separator(),
            status_texts: Vec::new(),
            tooltip: "Idlerx".into(),
            rx,
        })
    }

    pub fn commands(&self) -> Vec<Command> {
        self.rx.try_iter().collect()
    }

    pub fn set_tooltip(&mut self, text: &str) {
        // the notification area keeps at most 127 characters
        let text: String = text.chars().take(127).collect();
        if text != self.tooltip {
            let _ = self.icon.set_tooltip(Some(&text));
            self.tooltip = text;
        }
    }

    /// Red dot on the icon (a group's VPN is down).
    pub fn set_alert(&mut self, on: bool) {
        if on != self.alert_on {
            let icon = if on { &self.alert } else { &self.normal };
            let _ = self.icon.set_icon(Some(icon.clone()));
            self.alert_on = on;
        }
    }

    /// Lines at the top of the menu (VPN state of each group).
    pub fn set_status(&mut self, lines: Vec<String>) {
        if lines == self.status_texts {
            return;
        }
        for item in self.status.drain(..) {
            let _ = self.menu.remove(&item);
        }
        let _ = self.menu.remove(&self.separator);
        for (i, line) in lines.iter().enumerate() {
            let item = MenuItem::new(line, false, None);
            let _ = self.menu.insert(&item, i);
            self.status.push(item);
        }
        if !lines.is_empty() {
            let _ = self.menu.insert(&self.separator, lines.len());
        }
        self.status_texts = lines;
    }
}

/// Windows notification from the tray icon (`alert` = warning icon).
/// Respects "do not disturb". False if it could not be shown.
pub fn notify(text: &str, alert: bool) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_INFO, NIIF_INFO, NIIF_RESPECT_QUIET_TIME,
        NIIF_WARNING, NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};

    fn copy<const N: usize>(target: &mut [u16; N], text: &str) {
        for (t, v) in target
            .iter_mut()
            .zip(text.encode_utf16().take(N.saturating_sub(1)))
        {
            *t = v;
        }
    }
    unsafe {
        // the hidden window of the tray-icon crate (its class is pinned by
        // Cargo.lock; if it ever changes, notifications silently stop)
        let class = crate::single::wide("tray_icon_app");
        let me = GetCurrentProcessId();
        let mut hwnd = std::ptr::null_mut();
        loop {
            hwnd = FindWindowExW(std::ptr::null_mut(), hwnd, class.as_ptr(), std::ptr::null());
            if hwnd.is_null() {
                return false;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == me {
                break;
            }
        }
        for id in 1..=32 {
            let mut ident: NOTIFYICONIDENTIFIER = std::mem::zeroed();
            ident.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
            ident.hWnd = hwnd;
            ident.uID = id;
            let mut rect: RECT = std::mem::zeroed();
            if Shell_NotifyIconGetRect(&ident, &mut rect) != 0 {
                continue;
            }
            let mut n: NOTIFYICONDATAW = std::mem::zeroed();
            n.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            n.hWnd = hwnd;
            n.uID = id;
            n.uFlags = NIF_INFO;
            n.dwInfoFlags = if alert { NIIF_WARNING } else { NIIF_INFO } | NIIF_RESPECT_QUIET_TIME;
            n.Anonymous.uTimeout = 5_000;
            copy(&mut n.szInfoTitle, "Idlerx");
            copy(&mut n.szInfo, text);
            return Shell_NotifyIconW(NIM_MODIFY, &n) != 0;
        }
        false
    }
}

/// Shows or removes the window's taskbar button (the window stays
/// minimized while in the tray).
pub fn taskbar_button(hwnd: isize, visible: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
    if hwnd == 0 {
        return;
    }
    unsafe {
        let Ok(list) =
            CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        else {
            return;
        };
        if list.HrInit().is_err() {
            return;
        }
        let h = HWND(hwnd as _);
        let _ = if visible {
            list.AddTab(h)
        } else {
            list.DeleteTab(h)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_dot_on_the_icon() {
        let size = 64u32;
        let base = vec![0u8; (size * size * 4) as usize];
        let out = with_red_dot(&base, size);
        let px = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        assert_eq!(px(47, 47), [0xef, 0x44, 0x44, 0xff]); // center: red
        assert_eq!(px(2, 2), [0, 0, 0, 0]); // far corner: untouched
    }
}
