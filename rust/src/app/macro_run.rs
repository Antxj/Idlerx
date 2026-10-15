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
        // IDLERX_SCREENSHOT=record-test (tests): records the first account
        // while clicking it from here (input that counts as the user's), then
        // writes the recorded steps to the pages log
        if self.screenshot.as_deref() == Some("record-test") {
            self.record_test();
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

/// A macro being recorded on one account.
pub(super) struct Recording {
    pub id: String,
    pub recorder: macros::Recorder,
    asked: Instant,
}

impl App {
    pub(super) fn recording_on(&self, id: &str) -> bool {
        self.recording.as_ref().is_some_and(|r| r.id == id)
    }

    /// Steps recorded so far (shown on the stop button).
    pub(super) fn recorded_steps(&self) -> usize {
        self.recording
            .as_ref()
            .map_or(0, |r| r.recorder.steps.len())
    }

    pub(super) fn start_recording(&mut self, id: &str) {
        if self.recording.is_some() {
            self.stop_recording(false);
        }
        let Some(p) = self.panels.get(id) else {
            return;
        };
        p.record(true);
        self.stop_macro(id, "gravando uma macro nova");
        self.recording = Some(Recording {
            id: id.to_string(),
            recorder: macros::Recorder::default(),
            asked: Instant::now(),
        });
        self.note(id, "gravação de macro iniciada");
    }

    /// Ends the recording; `edit` opens the editor with what was recorded.
    pub(super) fn stop_recording(&mut self, edit: bool) {
        let Some(mut rec) = self.recording.take() else {
            return;
        };
        if let Some(p) = self.panels.get(&rec.id) {
            p.record(false);
            for ev in p.take_recorded() {
                rec.recorder.push(&ev);
            }
        }
        self.note(
            &rec.id,
            &format!("gravação parada: {} passo(s)", rec.recorder.steps.len()),
        );
        if edit {
            self.open_macro_editor_with(&rec.id, Some(rec.recorder.steps));
        }
    }

    /// Collects what the page reported while recording.
    pub(super) fn recording_tick(&mut self) {
        let Some(rec) = self.recording.as_mut() else {
            return;
        };
        let Some(p) = self.panels.get(&rec.id) else {
            // the account closed: nothing more to record
            self.recording = None;
            return;
        };
        for ev in p.take_recorded() {
            rec.recorder.push(&ev);
        }
        // a page loaded meanwhile starts with recording off: ask again
        if rec.asked.elapsed() > Duration::from_secs(2) {
            rec.asked = Instant::now();
            p.record(true);
        }
        self.ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl App {
    /// See IDLERX_SCREENSHOT=record-test in `macro_tick`.
    fn record_test(&mut self) {
        let Some(id) = self.cfg.display_order().into_iter().next() else {
            return;
        };
        let Some(p) = self.panels.get(&id) else {
            return;
        };
        let t = p.started.elapsed().as_secs_f32();
        let stage = self.macro_started_once.len();
        let at = [6.0, 7.0, 8.3, 9.0, 9.4, 11.0];
        if stage >= at.len() || t < at[stage] {
            return;
        }
        self.macro_started_once.insert(format!("stage{stage}"));
        let left = crate::config::MouseButton::Left;
        match stage {
            0 => self.start_recording(&id),
            1 => {
                p.click(0.25, 0.5, left, false);
            }
            2 => {
                p.click(0.75, 0.5, crate::config::MouseButton::Right, false);
            }
            3 => {
                if let Some(k) = crate::macros::key("F2") {
                    p.key(k);
                }
            }
            4 => {
                p.click(0.3, 0.4, left, true);
            }
            _ => {
                if let Some(rec) = self.recording.as_ref() {
                    let steps = serde_json::to_string(&rec.recorder.steps).unwrap_or_default();
                    self.note(&id, &format!("record-test: {steps}"));
                }
                self.stop_recording(false);
            }
        }
    }
}
