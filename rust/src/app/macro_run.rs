//! Playing the accounts' macros: each running one is advanced every frame
//! (the frame is asked for at the moment the next step is due, also while
//! the window is minimized or in the tray).

use super::*;
use crate::macros::{self, Input, Run};

impl App {
    pub(super) fn macro_running(&self, id: &str) -> bool {
        self.macro_runs.contains_key(id)
    }

    pub(super) fn start_macro(&mut self, id: &str) {
        let has_steps = self
            .cfg
            .account(id)
            .and_then(|a| a.macro_steps.as_ref())
            .is_some_and(|m| !m.steps.is_empty());
        if !has_steps || !self.panels.contains_key(id) {
            return;
        }
        self.macro_runs
            .insert(id.to_string(), Run::new(Instant::now()));
        self.note(id, "macro iniciada");
    }

    pub(super) fn stop_macro(&mut self, id: &str, why: &str) {
        if let Some(run) = self.macro_runs.remove(id) {
            self.note(
                id,
                &format!("macro parada: {why} ({} rodada(s) completas)", run.rounds),
            );
        }
    }

    pub(super) fn stop_all_macros(&mut self, why: &str) {
        let ids: Vec<String> = self.macro_runs.keys().cloned().collect();
        for id in ids {
            self.stop_macro(&id, why);
        }
    }

    /// Sends what is due and asks for a frame when the next step is.
    pub(super) fn macro_tick(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        // IDLERX_SCREENSHOT=start-macros (tests): every macro starts once its
        // page is open
        // IDLERX_SCREENSHOT=macro-editor: the first account's macro editor
        if self.screenshot.as_deref() == Some("macro-editor") && self.macro_started_once.is_empty()
        {
            let first = self.cfg.display_order().into_iter().find(|id| {
                self.panels
                    .get(id)
                    .is_some_and(|p| p.started.elapsed() > Duration::from_secs(5))
            });
            if let Some(id) = first {
                self.macro_started_once.insert(id.clone());
                self.open_macro_editor(&id);
            }
        }
        if self.screenshot.as_deref() == Some("start-macros") {
            // after the first load (a load stops the macro)
            let ids: Vec<String> = self
                .panels
                .iter()
                .filter(|(_, p)| p.started.elapsed() > Duration::from_secs(5))
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                if !self.macro_started_once.contains(&id) {
                    self.macro_started_once.insert(id.clone());
                    self.start_macro(&id);
                }
            }
        }
        if self.macro_runs.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut soonest: Option<Instant> = None;
        let mut finished = Vec::new();
        let ids: Vec<String> = self.macro_runs.keys().cloned().collect();
        for id in ids {
            let Some(m) = self.cfg.account(&id).and_then(|a| a.macro_steps.clone()) else {
                finished.push((id, "a macro foi apagada"));
                continue;
            };
            let Some(p) = self.panels.get(&id) else {
                finished.push((id, "a conta foi fechada"));
                continue;
            };
            let Some(run) = self.macro_runs.get_mut(&id) else {
                continue;
            };
            let (inputs, next) = run.tick(&m, now);
            for input in inputs {
                match input {
                    Input::Click {
                        x,
                        y,
                        button,
                        double,
                    } => {
                        p.click(x, y, button, double);
                    }
                    Input::Key(name) => {
                        if let Some(k) = macros::key(&name) {
                            p.key(k);
                        }
                    }
                }
            }
            match next {
                Some(t) => soonest = Some(soonest.map_or(t, |s| s.min(t))),
                None => finished.push((id, "terminou as rodadas")),
            }
        }
        for (id, why) in finished {
            self.stop_macro(&id, why);
        }
        if let Some(t) = soonest {
            ctx.request_repaint_after(t.saturating_duration_since(Instant::now()));
        }
    }
}
