//! Macro editor: a picture of the account's page to place the clicks on,
//! the list of steps (clicks, waits, keys) and how they repeat.
//!
//! The picture is taken before the dialog opens: a dialog hides the pages
//! (they are native windows that would cover it), and a hidden page cannot
//! be pictured.

use super::*;
use crate::config::{Macro, MouseButton, Step};
use crate::macros;

/// Wait after closing the dialog before picturing the page again (it has
/// to be shown and drawn first).
const SHOT_DELAY: Duration = Duration::from_millis(500);

pub(super) struct MacroEdit {
    id: String,
    name: String,
    m: Macro,
    picture: Option<egui::TextureHandle>,
    /// Button and double click of the clicks added on the picture.
    button: MouseButton,
    double: bool,
    /// Click step moved by the next click on the picture.
    moving: Option<usize>,
}

/// A picture on its way: the editor waiting for it, and the result.
pub(super) struct Shot {
    edit: MacroEdit,
    asked_at: Instant,
    asked: bool,
    result: Rc<RefCell<Option<Option<Vec<u8>>>>>,
}

impl App {
    /// Takes a picture of the page, then opens the editor.
    pub(super) fn open_macro_editor(&mut self, id: &str) {
        self.open_macro_editor_with(id, None);
    }

    /// The editor with these steps instead of the saved ones (a recording).
    pub(super) fn open_macro_editor_with(&mut self, id: &str, steps: Option<Vec<Step>>) {
        let Some(a) = self.cfg.account(id) else {
            return;
        };
        let mut m = a.macro_steps.clone().unwrap_or_default();
        if let Some(steps) = steps {
            m.steps = steps;
        }
        let edit = MacroEdit {
            id: id.to_string(),
            name: a.name.clone(),
            m,
            picture: None,
            button: MouseButton::Left,
            double: false,
            moving: None,
        };
        self.shoot(edit, false);
    }

    /// Asks for a picture (`wait`: the dialog was just closed, give the
    /// page a moment to be on screen again).
    fn shoot(&mut self, edit: MacroEdit, wait: bool) {
        self.dialog = None;
        self.macro_shot = Some(Shot {
            edit,
            asked_at: if wait {
                Instant::now()
            } else {
                Instant::now() - SHOT_DELAY
            },
            asked: false,
            result: Rc::new(RefCell::new(None)),
        });
        self.ctx.request_repaint();
    }

    /// Called every frame: asks for the picture when it is time, and opens
    /// the editor once it arrived (or could not be taken).
    pub(super) fn macro_shot_ready(&mut self) {
        let Some(shot) = self.macro_shot.as_mut() else {
            return;
        };
        if !shot.asked {
            if shot.asked_at.elapsed() < SHOT_DELAY {
                self.ctx.request_repaint_after(SHOT_DELAY);
                return;
            }
            shot.asked = true;
            match self.panels.get(&shot.edit.id) {
                Some(p) => {
                    let slot = shot.result.clone();
                    let ctx = self.ctx.clone();
                    p.capture(move |png| {
                        *slot.borrow_mut() = Some(png);
                        ctx.request_repaint();
                    });
                    return;
                }
                // paused account: the editor opens without a picture
                None => *shot.result.borrow_mut() = Some(None),
            }
        }
        let Some(png) = shot.result.borrow_mut().take() else {
            return;
        };
        let Some(mut shot) = self.macro_shot.take() else {
            return;
        };
        shot.edit.picture = png.and_then(|data| {
            let img = image::load_from_memory(&data).ok()?.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            Some(
                self.ctx
                    .load_texture("macro_picture", color, egui::TextureOptions::LINEAR),
            )
        });
        self.dialog = Some(Dialog::Macro(shot.edit));
    }

    /// The editor; returns false when it closes.
    pub(super) fn macro_dialog(&mut self, ctx: &egui::Context, st: &mut MacroEdit) -> bool {
        let running = self.macro_running(&st.id);
        let mut keep = true;
        let mut save = false;
        let mut start = false;
        let mut retake = false;
        let mut record = false;
        let modal = egui::Modal::new(egui::Id::new("macro")).show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.set_width(660.0);
            ui.heading(trf!("Macro — {}", "Macro — {}", st.name));
            ui.label(
                RichText::new(tr!(
                    "Cliques e teclas vão direto para a página desta conta: o mouse e o \
                     teclado do Windows ficam livres, e funciona com a conta escondida.",
                    "Clicks and keys go straight to this account's page: the Windows mouse \
                     and keyboard stay free, and it works with the account hidden."
                ))
                .small()
                .color(weak(dark)),
            );
            ui.add_space(6.0);
            picture(ui, st);
            ui.horizontal(|ui| {
                ui.label(tr!(
                    "Clique na foto para adicionar:",
                    "Click the picture to add:"
                ));
                egui::ComboBox::from_id_salt("macro_button")
                    .selected_text(button_name(st.button))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for b in [MouseButton::Left, MouseButton::Right] {
                            ui.selectable_value(&mut st.button, b, button_name(b));
                        }
                    });
                ui.checkbox(&mut st.double, tr!("duplo", "double"));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(tr!("Tirar outra foto", "Take another picture"))
                        .clicked()
                    {
                        retake = true;
                    }
                    if ui
                        .button(RichText::new(tr!("⏺ Gravar", "⏺ Record")).color(DANGER))
                        .on_hover_text(tr!(
                            "Fecha o editor; use o jogo normalmente: seus cliques, teclas e o \
                             tempo entre eles viram os passos (substituem os atuais).",
                            "Closes the editor; use the game normally: your clicks, keys and \
                             the time between them become the steps (replacing the current ones)."
                        ))
                        .clicked()
                    {
                        record = true;
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            steps_list(ui, st);
            ui.horizontal(|ui| {
                if ui.button(tr!("+ Espera", "+ Wait")).clicked() {
                    st.m.steps.push(Step::Wait { secs: 1.0 });
                }
                if ui.button(tr!("+ Tecla", "+ Key")).clicked() {
                    st.m.steps.push(Step::Key {
                        key: macros::KEYS[0].name.to_string(),
                    });
                }
            });
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(tr!("Repetir a cada", "Repeat every"));
                ui.add(time_field(&mut st.m.every_secs, 0.0));
                ui.label(tr!(
                    "depois do último passo · rodadas:",
                    "after the last step · rounds:"
                ));
                ui.add(egui::DragValue::new(&mut st.m.times).range(0..=1_000_000));
                ui.label(
                    RichText::new(tr!("(0 = até parar)", "(0 = until stopped)")).color(weak(dark)),
                );
            });
            ui.label(
                RichText::new(tr!(
                    "A macro para sozinha se a página recarregar, a conta fechar ou a VPN \
                     do grupo cair.",
                    "The macro stops by itself if the page reloads, the account closes or \
                     the group's VPN drops."
                ))
                .small()
                .color(weak(dark)),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let has = !st.m.steps.is_empty();
                if ui
                    .add_enabled(
                        has,
                        egui::Button::new(
                            RichText::new(if running {
                                tr!("Salvar e reiniciar", "Save and restart")
                            } else {
                                tr!("Salvar e iniciar", "Save and start")
                            })
                            .color(Color32::WHITE),
                        )
                        .fill(ACCENT),
                    )
                    .clicked()
                {
                    save = true;
                    start = true;
                }
                if ui.button(tr!("Salvar", "Save")).clicked() {
                    save = true;
                }
                if ui.button(tr!("Cancelar", "Cancel")).clicked() {
                    keep = false;
                }
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if save {
            let m = (!st.m.steps.is_empty()).then(|| st.m.clone());
            let old = self.cfg.account(&st.id).and_then(|a| a.macro_steps.clone());
            if let Some(a) = self.cfg.account_mut(&st.id) {
                a.macro_steps = m.clone();
            }
            self.mark_dirty();
            match (start, m, old, self.macro_runs.get_mut(&st.id)) {
                // just "Save": a running macro goes on where it was, with
                // the new steps and times
                (false, Some(new), Some(old), Some(run)) => {
                    run.edited(&old, &new, Instant::now());
                    self.note(&st.id, "macro editada (continua rodando)");
                }
                _ => {
                    self.stop_macro(&st.id, "macro editada");
                    if start {
                        self.start_macro(&st.id);
                    }
                }
            }
            keep = false;
        }
        if record {
            self.dialog = None;
            self.start_recording(&st.id);
            return false;
        }
        if retake {
            let edit = MacroEdit {
                id: st.id.clone(),
                name: st.name.clone(),
                m: st.m.clone(),
                picture: None,
                button: st.button,
                double: st.double,
                moving: None,
            };
            self.shoot(edit, true);
            return false;
        }
        keep
    }
}

fn button_name(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => tr!("esquerdo", "left"),
        MouseButton::Right => tr!("direito", "right"),
    }
}

/// The page picture with the clicks on it; a click adds a step (or moves
/// the one being moved).
fn picture(ui: &mut egui::Ui, st: &mut MacroEdit) {
    let dark = ui.visuals().dark_mode;
    let Some(tex) = &st.picture else {
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), Sense::hover());
        ui.painter()
            .rect_filled(r, CornerRadius::same(6), weak(dark).gamma_multiply(0.15));
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            tr!(
                "Sem foto: a conta precisa estar aberta e na tela. Abra-a e use \"Tirar outra foto\".",
                "No picture: the account must be open and on screen. Open it and use \"Take another picture\"."
            ),
            FontId::proportional(13.0),
            weak(dark),
        );
        return;
    };
    let [w, h] = tex.size();
    let max = Vec2::new(ui.available_width(), 360.0);
    let scale = (max.x / w as f32).min(max.y / h as f32).min(1.0);
    let size = Vec2::new(w as f32 * scale, h as f32 * scale);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    painter.image(
        tex.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    let mut n = 0;
    for (i, step) in st.m.steps.iter().enumerate() {
        if let Step::Click { x, y, button, .. } = step {
            n += 1;
            let c = Pos2::new(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            );
            let color = if st.moving == Some(i) {
                FAVORITE
            } else if *button == MouseButton::Right {
                DANGER
            } else {
                ACCENT
            };
            painter.circle(c, 9.0, color, Stroke::new(1.5, Color32::WHITE));
            painter.text(
                c,
                Align2::CENTER_CENTER,
                (i + 1).to_string(),
                FontId::proportional(10.0),
                Color32::WHITE,
            );
        }
    }
    let _ = n;
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let fx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let fy = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        match st.moving.take() {
            Some(i) => {
                if let Some(Step::Click { x, y, .. }) = st.m.steps.get_mut(i) {
                    *x = fx;
                    *y = fy;
                }
            }
            None => st.m.steps.push(Step::Click {
                x: fx,
                y: fy,
                button: st.button,
                double: st.double,
            }),
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::Crosshair);
}

/// The steps, editable, with up/down/remove.
fn steps_list(ui: &mut egui::Ui, st: &mut MacroEdit) {
    let dark = ui.visuals().dark_mode;
    if st.m.steps.is_empty() {
        ui.label(
            RichText::new(tr!(
                "Nenhum passo ainda: clique na foto onde a macro deve clicar.",
                "No steps yet: click the picture where the macro should click."
            ))
            .color(weak(dark)),
        );
        return;
    }
    let mut remove = None;
    let mut swap = None;
    let len = st.m.steps.len();
    egui::ScrollArea::vertical()
        .max_height(190.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, step) in st.m.steps.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).strong());
                    match step {
                        Step::Click {
                            x,
                            y,
                            button,
                            double,
                        } => {
                            ui.label(trf!(
                                "Clicar em {:.0}% , {:.0}%",
                                "Click at {:.0}% , {:.0}%",
                                *x * 100.0,
                                *y * 100.0
                            ));
                            egui::ComboBox::from_id_salt(("macro_step_button", i))
                                .selected_text(button_name(*button))
                                .width(80.0)
                                .show_ui(ui, |ui| {
                                    for b in [MouseButton::Left, MouseButton::Right] {
                                        ui.selectable_value(button, b, button_name(b));
                                    }
                                });
                            ui.checkbox(double, tr!("duplo", "double"));
                            let moving = st.moving == Some(i);
                            if ui
                                .selectable_label(moving, tr!("mover", "move"))
                                .on_hover_text(tr!(
                                    "Depois clique na foto, no lugar novo.",
                                    "Then click the picture at the new spot."
                                ))
                                .clicked()
                            {
                                st.moving = if moving { None } else { Some(i) };
                            }
                        }
                        Step::Wait { secs } => {
                            ui.label(tr!("Esperar", "Wait"));
                            ui.add(time_field(secs, 0.1));
                        }
                        Step::Key { key } => {
                            ui.label(tr!("Tecla", "Key"));
                            egui::ComboBox::from_id_salt(("macro_step_key", i))
                                .selected_text(key.as_str())
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    for k in macros::KEYS {
                                        ui.selectable_value(key, k.name.to_string(), k.name);
                                    }
                                });
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("🗑").clicked() {
                            remove = Some(i);
                        }
                        if ui
                            .add_enabled(i + 1 < len, egui::Button::new("⏷").small())
                            .clicked()
                        {
                            swap = Some((i, i + 1));
                        }
                        if ui
                            .add_enabled(i > 0, egui::Button::new("⏶").small())
                            .clicked()
                        {
                            swap = Some((i, i - 1));
                        }
                    });
                });
            }
        });
    if let Some((a, b)) = swap {
        st.m.steps.swap(a, b);
        st.moving = None;
    }
    if let Some(i) = remove {
        st.m.steps.remove(i);
        st.moving = None;
    }
}

/// A time field in seconds that shows and takes minutes and hours too
/// ("50 min", "1h30", "90 s"; a plain number is seconds).
fn time_field(secs: &mut f32, min: f32) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let speed = if *secs >= 600.0 {
            10.0
        } else if *secs >= 60.0 {
            1.0
        } else {
            0.1
        };
        ui.add(
            egui::DragValue::new(secs)
                .range(min..=86_400.0)
                .speed(speed)
                .custom_formatter(|v, _| macros::fmt_secs(v))
                .custom_parser(macros::parse_secs),
        )
        .on_hover_text(tr!(
            "Clique para digitar: 90 s, 50 min, 1h30, 2 h…",
            "Click to type: 90 s, 50 min, 1h30, 2 h…"
        ))
    }
}
