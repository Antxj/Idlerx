//! Idlerx: several isolated browser sessions (accounts) side by side in a
//! single window - made for idle games with more than one account.

// no console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod app;
mod blocker;
mod config;
mod events;
mod macros;
mod metrics;
mod panels;
mod popup;
mod single;
mod tray;
mod update;
mod vpn;

use eframe::egui;
use std::time::Duration;

pub const ICON_RGBA: &[u8] = include_bytes!("../assets/icon_64.rgba");

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // elevated helper (UAC): authorizes the user for the OpenVPN service
    if let Some(user) = args
        .iter()
        .position(|a| a == vpn::service::ARG_AUTHORIZE)
        .and_then(|i| args.get(i + 1))
    {
        std::process::exit(vpn::service::add_to_group(user));
    }
    // after "Update now", the previous version opens this one passing its PID
    if let Some(pid) = args
        .iter()
        .position(|a| a == update::ARG_AFTER_UPDATE)
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
    {
        update::wait_for_process(pid, Duration::from_secs(30));
    }

    if !single::acquire_or_signal() {
        return Ok(()); // already running: the other instance shows itself
    }
    single::spawn_show_listener();
    update::clean_leftovers();

    let cfg = config::load();
    i18n::apply(cfg.language.as_deref().and_then(i18n::Language::from_code));

    let size = cfg.window_size.unwrap_or([1280.0, 800.0]);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Idlerx")
        .with_app_id("Idlerx")
        .with_inner_size(size)
        .with_min_inner_size([720.0, 480.0])
        // a maximized window is created hidden at its normal size and only
        // maximized and shown on the first frame: maximizing at creation let
        // the saved size win, leaving a "maximized" window that does not
        // fill the screen
        .with_visible(!cfg.window_maximized)
        .with_icon(egui::IconData {
            rgba: ICON_RGBA.to_vec(),
            width: 64,
            height: 64,
        });
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    let result = eframe::run_native(
        "Idlerx",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg)))),
    );
    if let Err(e) = &result {
        report_startup_error(e);
    }
    result
}

/// The window could not be created (graphics driver, etc.): there is no
/// console in release builds, so the error goes to a message box and a log.
fn report_startup_error(e: &eframe::Error) {
    let text = format!("{e}");
    let dir = config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("startup-error.log"), &text);
    let msg = trf!(
        "O Idlerx não conseguiu abrir a janela:\n\n{text}",
        "Idlerx could not open its window:\n\n{text}"
    );
    let title = single::wide("Idlerx");
    let body = single::wide(&msg);
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}
