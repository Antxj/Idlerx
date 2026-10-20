//! Macros: steps (clicks, keys, waits) repeated on an account's page.
//!
//! The input goes straight to the page through the browser's DevTools
//! protocol (see `panels`): the Windows mouse and keyboard are not used, so
//! the computer stays free, and it works with the account hidden (another
//! group in the grid, window minimized or in the tray).
//!
//! `Run` only decides what is due and when; the app sends it.

use crate::config::{Macro, MouseButton, Step};
use std::time::{Duration, Instant};

/// Pause between two input steps in a row (the page handles each one).
const INPUT_GAP: Duration = Duration::from_millis(120);

/// A key a macro can press.
pub struct Key {
    /// Name shown and saved.
    pub name: &'static str,
    /// DOM `key` and `code`, Windows virtual-key code, text typed (if any).
    pub key: &'static str,
    pub code: &'static str,
    pub vk: u32,
    pub text: Option<&'static str>,
}

macro_rules! keys {
    ($($name:literal, $key:literal, $code:literal, $vk:literal, $text:expr;)*) => {
        &[$(Key { name: $name, key: $key, code: $code, vk: $vk, text: $text },)*]
    };
}

pub const KEYS: &[Key] = keys![
    "Espaço", " ", "Space", 0x20, Some(" ");
    "Enter", "Enter", "Enter", 0x0D, Some("\r");
    "Esc", "Escape", "Escape", 0x1B, None;
    "Tab", "Tab", "Tab", 0x09, None;
    "Backspace", "Backspace", "Backspace", 0x08, None;
    "↑", "ArrowUp", "ArrowUp", 0x26, None;
    "↓", "ArrowDown", "ArrowDown", 0x28, None;
    "←", "ArrowLeft", "ArrowLeft", 0x25, None;
    "→", "ArrowRight", "ArrowRight", 0x27, None;
    "F1", "F1", "F1", 0x70, None;
    "F2", "F2", "F2", 0x71, None;
    "F3", "F3", "F3", 0x72, None;
    "F4", "F4", "F4", 0x73, None;
    "F5", "F5", "F5", 0x74, None;
    "F6", "F6", "F6", 0x75, None;
    "F7", "F7", "F7", 0x76, None;
    "F8", "F8", "F8", 0x77, None;
    "F9", "F9", "F9", 0x78, None;
    "F10", "F10", "F10", 0x79, None;
    "F11", "F11", "F11", 0x7A, None;
    "F12", "F12", "F12", 0x7B, None;
    "1", "1", "Digit1", 0x31, Some("1");
    "2", "2", "Digit2", 0x32, Some("2");
    "3", "3", "Digit3", 0x33, Some("3");
    "4", "4", "Digit4", 0x34, Some("4");
    "5", "5", "Digit5", 0x35, Some("5");
    "6", "6", "Digit6", 0x36, Some("6");
    "7", "7", "Digit7", 0x37, Some("7");
    "8", "8", "Digit8", 0x38, Some("8");
    "9", "9", "Digit9", 0x39, Some("9");
    "0", "0", "Digit0", 0x30, Some("0");
    "A", "a", "KeyA", 0x41, Some("a");
    "B", "b", "KeyB", 0x42, Some("b");
    "C", "c", "KeyC", 0x43, Some("c");
    "D", "d", "KeyD", 0x44, Some("d");
    "E", "e", "KeyE", 0x45, Some("e");
    "F", "f", "KeyF", 0x46, Some("f");
    "G", "g", "KeyG", 0x47, Some("g");
    "H", "h", "KeyH", 0x48, Some("h");
    "I", "i", "KeyI", 0x49, Some("i");
    "J", "j", "KeyJ", 0x4A, Some("j");
    "K", "k", "KeyK", 0x4B, Some("k");
    "L", "l", "KeyL", 0x4C, Some("l");
    "M", "m", "KeyM", 0x4D, Some("m");
    "N", "n", "KeyN", 0x4E, Some("n");
    "O", "o", "KeyO", 0x4F, Some("o");
    "P", "p", "KeyP", 0x50, Some("p");
    "Q", "q", "KeyQ", 0x51, Some("q");
    "R", "r", "KeyR", 0x52, Some("r");
    "S", "s", "KeyS", 0x53, Some("s");
    "T", "t", "KeyT", 0x54, Some("t");
    "U", "u", "KeyU", 0x55, Some("u");
    "V", "v", "KeyV", 0x56, Some("v");
    "W", "w", "KeyW", 0x57, Some("w");
    "X", "x", "KeyX", 0x58, Some("x");
    "Y", "y", "KeyY", 0x59, Some("y");
    "Z", "z", "KeyZ", 0x5A, Some("z");
];

pub fn key(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// Something to send to the page now.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Click {
        x: f32,
        y: f32,
        button: MouseButton,
        double: bool,
    },
    Key(String),
}

/// A macro being played on one account.
#[derive(Clone, Debug)]
pub struct Run {
    step: usize,
    next: Instant,
    /// Rounds finished.
    pub rounds: u32,
}

impl Run {
    pub fn new(now: Instant) -> Run {
        Run {
            step: 0,
            next: now,
            rounds: 0,
        }
    }

    /// The input due by `now`, and when to look again (None = the macro
    /// is over: the set number of rounds is done, or it has no steps).
    pub fn tick(&mut self, m: &Macro, now: Instant) -> (Vec<Input>, Option<Instant>) {
        let mut out = Vec::new();
        if m.steps.is_empty() {
            return (out, None);
        }
        // a long freeze (sleep) does not fire a burst of late steps
        let mut guard = 0;
        while now >= self.next && guard < m.steps.len() + 1 {
            guard += 1;
            let step = &m.steps[self.step.min(m.steps.len() - 1)];
            self.next = match step {
                Step::Click {
                    x,
                    y,
                    button,
                    double,
                } => {
                    out.push(Input::Click {
                        x: *x,
                        y: *y,
                        button: *button,
                        double: *double,
                    });
                    now + INPUT_GAP
                }
                Step::Key { key } => {
                    out.push(Input::Key(key.clone()));
                    now + INPUT_GAP
                }
                Step::Wait { secs } => now + Duration::from_secs_f32(secs.max(0.0)),
            };
            self.step += 1;
            if self.step >= m.steps.len() {
                self.step = 0;
                self.rounds += 1;
                if m.times > 0 && self.rounds >= m.times {
                    return (out, None);
                }
                self.next = self
                    .next
                    .max(now + Duration::from_secs_f32(m.every_secs.max(0.0)));
            }
            if !out.is_empty() {
                // one input at a time: the page gets a moment between them
                break;
            }
        }
        (out, Some(self.next))
    }

    /// When the next click or key will be sent (None = no more: the
    /// macro has no click or key, or it ends first).
    pub fn next_input(&self, m: &Macro) -> Option<Instant> {
        let len = m.steps.len();
        if !m.steps.iter().any(|s| !matches!(s, Step::Wait { .. })) {
            return None;
        }
        let (mut t, mut i, mut rounds) = (self.next, self.step.min(len - 1), self.rounds);
        // at most one full round and a bit
        for _ in 0..=len * 2 {
            let Step::Wait { secs } = &m.steps[i] else {
                return Some(t);
            };
            // as `tick` does: after a step, its wait; after the last one,
            // at least the pause between rounds
            let at = t;
            t = at + Duration::from_secs_f32(secs.max(0.0));
            i += 1;
            if i >= len {
                i = 0;
                rounds += 1;
                if m.times > 0 && rounds >= m.times {
                    return None;
                }
                t = t.max(at + Duration::from_secs_f32(m.every_secs.max(0.0)));
            }
        }
        None
    }
}

/// A time span in seconds as people say it: "1.5 s", "50 min",
/// "1 h 30 min", "2 min 5 s".
pub fn fmt_secs(secs: f64) -> String {
    if secs < 60.0 {
        let r = (secs * 10.0).round() / 10.0;
        return if r.fract() == 0.0 {
            format!("{r:.0} s")
        } else {
            format!("{r:.1} s")
        };
    }
    let total = secs.round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} h"));
    }
    if m > 0 {
        parts.push(format!("{m} min"));
    }
    if s > 0 {
        parts.push(format!("{s} s"));
    }
    parts.join(" ")
}

/// Reads what was typed in a time field: a plain number is seconds; with
/// units, any mix of hours, minutes and seconds ("50m", "50 min",
/// "1h30", "1h 30m 5s", "2,5 s"). A number after hours without a unit
/// means minutes ("1h30").
pub fn parse_secs(text: &str) -> Option<f64> {
    let t = text.trim().to_lowercase().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let mut total = 0.0;
    let mut last_unit = 0.0;
    let mut rest = t.as_str();
    while !rest.is_empty() {
        rest = rest.trim_start();
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let num: f64 = rest[..end].parse().ok()?;
        rest = rest[end..].trim_start();
        let uend = rest
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(rest.len());
        let unit = match &rest[..uend] {
            "h" | "hr" | "hora" | "horas" | "hour" | "hours" => 3600.0,
            "m" | "min" | "mins" | "minuto" | "minutos" | "minute" | "minutes" => 60.0,
            "s" | "seg" | "sec" | "secs" | "segundo" | "segundos" | "second" | "seconds" => 1.0,
            // no unit: the one after the previous ("1h30" = 1 h 30 min)
            "" if last_unit == 3600.0 => 60.0,
            "" if last_unit == 60.0 => 1.0,
            _ => return None,
        };
        rest = &rest[uend..];
        total += num * unit;
        last_unit = unit;
    }
    Some(total)
}

/// Turns what the page reported while recording into steps: clicks, keys,
/// and the time the user took between them as waits.
#[derive(Default)]
pub struct Recorder {
    pub steps: Vec<Step>,
    /// Time of the last recorded action (ms, the page's clock).
    last: Option<f64>,
}

impl Recorder {
    /// One event from the page (see fps.js); anything malformed is ignored.
    pub fn push(&mut self, ev: &serde_json::Value) {
        let Some(t) = ev["t"].as_f64() else {
            return;
        };
        let step = match ev["kind"].as_str() {
            Some("click") => {
                let (Some(x), Some(y)) = (ev["x"].as_f64(), ev["y"].as_f64()) else {
                    return;
                };
                let button = if ev["button"].as_str() == Some("right") {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                let detail = ev["detail"].as_u64().unwrap_or(1);
                if detail >= 2 {
                    // the second press of a double click marks the first one
                    if let Some(Step::Click {
                        button: b, double, ..
                    }) = self.steps.last_mut()
                    {
                        if *b == button && detail == 2 {
                            *double = true;
                        }
                    }
                    return;
                }
                Step::Click {
                    x: (x as f32).clamp(0.0, 1.0),
                    y: (y as f32).clamp(0.0, 1.0),
                    button,
                    double: false,
                }
            }
            Some("key") => {
                let code = ev["code"].as_str().unwrap_or_default();
                let Some(k) = KEYS.iter().find(|k| k.code == code) else {
                    return; // a key macros cannot press (Shift, Ctrl...)
                };
                Step::Key {
                    key: k.name.to_string(),
                }
            }
            _ => return,
        };
        if let Some(last) = self.last {
            // the time taken, to a tenth of a second
            let secs = (((t - last) / 100.0).round() / 10.0) as f32;
            if secs >= 0.1 {
                self.steps.push(Step::Wait { secs });
            }
        }
        self.last = Some(t);
        self.steps.push(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_text() {
        assert_eq!(fmt_secs(3000.0), "50 min");
        assert_eq!(fmt_secs(5400.0), "1 h 30 min");
        assert_eq!(fmt_secs(125.0), "2 min 5 s");
        assert_eq!(fmt_secs(1.5), "1.5 s");
        assert_eq!(fmt_secs(5.0), "5 s");
        assert_eq!(parse_secs("3000"), Some(3000.0));
        assert_eq!(parse_secs("50m"), Some(3000.0));
        assert_eq!(parse_secs("50 min"), Some(3000.0));
        assert_eq!(parse_secs("1h30"), Some(5400.0));
        assert_eq!(parse_secs("1 h 30 min"), Some(5400.0));
        assert_eq!(parse_secs("1h 30m 5s"), Some(5405.0));
        assert_eq!(parse_secs("2,5 s"), Some(2.5));
        assert_eq!(parse_secs("abc"), None);
        for v in [0.5, 7.0, 3000.0, 5405.0, 86_400.0] {
            assert_eq!(parse_secs(&fmt_secs(v)), Some(v));
        }
    }

    #[test]
    fn next_input_time() {
        let t0 = Instant::now();
        let m = Macro {
            steps: vec![click(0.5), Step::Wait { secs: 10.0 }],
            every_secs: 3000.0,
            times: 0,
        };
        let mut run = Run::new(t0);
        assert_eq!(run.next_input(&m), Some(t0));
        run.tick(&m, t0); // the click
        let later = t0 + INPUT_GAP;
        // the wait ends the round: then the pause between rounds
        assert_eq!(run.next_input(&m), Some(later + Duration::from_secs(3000)));
        let once = Macro {
            times: 1,
            ..m.clone()
        };
        let mut run = Run::new(t0);
        run.tick(&once, t0);
        assert_eq!(run.next_input(&once), None);
    }

    fn click(x: f32) -> Step {
        Step::Click {
            x,
            y: 0.5,
            button: MouseButton::Left,
            double: false,
        }
    }

    #[test]
    fn steps_in_order_and_rounds() {
        let m = Macro {
            steps: vec![
                click(0.1),
                Step::Wait { secs: 2.0 },
                Step::Key { key: "F1".into() },
            ],
            every_secs: 10.0,
            times: 2,
        };
        let t0 = Instant::now();
        let mut r = Run::new(t0);
        // first click right away
        let (out, next) = r.tick(&m, t0);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Input::Click { x, .. } if x == 0.1));
        let next = next.unwrap();
        // nothing before it is due
        assert!(r.tick(&m, t0).0.is_empty());
        // the wait, then the key
        let (out, next) = r.tick(&m, next);
        assert!(out.is_empty());
        let next = next.unwrap();
        assert!(next >= t0 + Duration::from_secs(2));
        let (out, next) = r.tick(&m, next);
        assert_eq!(out, vec![Input::Key("F1".into())]);
        assert_eq!(r.rounds, 1);
        // the pause between rounds
        let next = next.unwrap();
        assert!(next >= t0 + Duration::from_secs(12));
        // second (last) round
        let (_, n) = r.tick(&m, next);
        let (_, n) = r.tick(&m, n.unwrap());
        let (out, n) = r.tick(&m, n.unwrap());
        assert_eq!(out.len(), 1);
        assert!(n.is_none(), "two rounds were asked");
    }

    #[test]
    fn recording_to_steps() {
        use serde_json::json;
        let mut r = Recorder::default();
        r.push(&json!({"kind":"click","x":0.25,"y":0.5,"button":"left","detail":1,"t":1000.0}));
        // 1.26 s later, a double click
        r.push(&json!({"kind":"click","x":0.5,"y":0.5,"button":"left","detail":1,"t":2260.0}));
        r.push(&json!({"kind":"click","x":0.5,"y":0.5,"button":"left","detail":2,"t":2400.0}));
        // a key the macro knows, and one it does not
        r.push(&json!({"kind":"key","code":"F2","t":3000.0}));
        r.push(&json!({"kind":"key","code":"ShiftLeft","t":3100.0}));
        r.push(&json!({"kind":"key","code":"KeyA","t":3040.0}));
        r.push(&json!({"bad":true}));
        assert_eq!(
            r.steps,
            vec![
                Step::Click {
                    x: 0.25,
                    y: 0.5,
                    button: MouseButton::Left,
                    double: false
                },
                Step::Wait { secs: 1.3 },
                Step::Click {
                    x: 0.5,
                    y: 0.5,
                    button: MouseButton::Left,
                    double: true
                },
                Step::Wait { secs: 0.7 },
                Step::Key { key: "F2".into() },
                Step::Key { key: "A".into() },
            ]
        );
    }

    #[test]
    fn empty_macro_ends() {
        let mut r = Run::new(Instant::now());
        assert_eq!(r.tick(&Macro::default(), Instant::now()).1, None);
    }

    #[test]
    fn known_keys() {
        assert_eq!(key("F5").unwrap().vk, 0x74);
        assert_eq!(key("Espaço").unwrap().code, "Space");
        assert!(key("nope").is_none());
        // names are unique
        for (i, k) in KEYS.iter().enumerate() {
            assert!(KEYS[i + 1..].iter().all(|o| o.name != k.name), "{}", k.name);
        }
    }
}
