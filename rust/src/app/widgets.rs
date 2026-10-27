//! Look and small pieces shared by the interface: colors, avatar, text,
//! tooltips that stay visible, the grid shape picture.

use super::*;

/// Areas egui draws above everything (menus, drop-downs, modal dialogs).
/// Tooltips are left out: hiding a page for a tooltip would only flicker.
pub(super) fn overlay_rects(ctx: &egui::Context) -> Vec<Rect> {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Foreground)
            .filter_map(|l| m.area_rect(l.id))
            .collect()
    })
}

/// Tooltips of the side list. A normal tooltip follows the pointer and can
/// spill over the pages, which cover it; these stay inside the side list.
pub(super) trait SideTip {
    fn side_tip(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl SideTip for egui::Response {
    fn side_tip(self, text: impl Into<egui::WidgetText>) -> Self {
        // a menu is open (right click): the tooltip would sit on top of it
        if egui::Popup::is_any_open(&self.ctx) {
            return self;
        }
        let band = Rect::from_x_y_ranges(6.0..=SIDEBAR_WIDTH - 6.0, self.rect.y_range());
        let mut tip = egui::Tooltip::for_enabled(&self);
        tip.popup = tip
            .popup
            .anchor(band)
            .align(egui::RectAlign::BOTTOM_START)
            .align_alternatives(&[egui::RectAlign::TOP_START])
            .width(SIDEBAR_WIDTH - 28.0);
        let text = text.into();
        tip.show(|ui| {
            // full width from the first frame: long words (file names)
            // wrap inside it instead of spilling out of a narrow box
            ui.set_min_width(SIDEBAR_WIDTH - 40.0);
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
        });
        self
    }
}

/// Right-click menu of a side list row, opened right below the row and
/// inside the list: a menu over a page would hide that page while open (the
/// pages are native windows that would cover it).
pub(super) fn side_menu(resp: &egui::Response, add_contents: impl FnOnce(&mut egui::Ui)) {
    let band = Rect::from_x_y_ranges(6.0..=SIDEBAR_WIDTH - 6.0, resp.rect.y_range());
    egui::Popup::context_menu(resp)
        .anchor(band)
        .align(egui::RectAlign::BOTTOM_START)
        .align_alternatives(&[egui::RectAlign::TOP_START])
        .width(SIDEBAR_WIDTH - 36.0)
        .show(add_contents);
}

/// Tooltips of the top bar and of the grid cell headers: shown beside the
/// button, inside the bar, instead of below it (where a page would cover them).
pub(super) trait BarTip {
    fn bar_tip(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl BarTip for egui::Response {
    fn bar_tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let mut tip = egui::Tooltip::for_enabled(&self);
        tip.popup = tip
            .popup
            .align(egui::RectAlign::LEFT)
            .align_alternatives(&[egui::RectAlign::RIGHT])
            .gap(6.0)
            .width(420.0);
        let text = text.into();
        tip.show(|ui| {
            ui.label(text);
        });
        self
    }
}

/// Small green leaf (economy mode), drawn so it does not depend on the font.
pub(super) fn leaf(ui: &mut egui::Ui) -> egui::Response {
    const GREEN: Color32 = Color32::from_rgb(0x4a, 0xde, 0x80);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let c = rect.center();
    let tip = c + Vec2::new(5.0, -5.0);
    let base = c + Vec2::new(-4.0, 4.0);
    // two curved edges between the base and the tip
    let mut points = Vec::new();
    for i in 0..=8 {
        let t = i as f32 / 8.0;
        let bulge = (t * std::f32::consts::PI).sin() * 3.2;
        points.push(base + (tip - base) * t + Vec2::new(-bulge, -bulge));
    }
    for i in (1..8).rev() {
        let t = i as f32 / 8.0;
        let bulge = (t * std::f32::consts::PI).sin() * 3.2;
        points.push(base + (tip - base) * t + Vec2::new(bulge, bulge));
    }
    let painter = ui.painter();
    painter.add(egui::Shape::convex_polygon(points, GREEN, Stroke::NONE));
    // stem and middle vein
    painter.line_segment([base + Vec2::new(-2.0, 2.0), base], Stroke::new(1.3, GREEN));
    painter.line_segment(
        [base, tip],
        Stroke::new(0.8, panel_bg(ui.visuals().dark_mode)),
    );
    resp
}

/// "CPU 3% · RAM 245 MB"
pub(super) fn usage_text(u: metrics::Usage) -> String {
    format!("CPU {:.0}% · RAM {}", u.cpu, metrics::fmt_memory(u.memory))
}

/// Name of a grid shape: "Auto", "3 col." or "2×2".
pub(super) fn shape_label(cols: u8, rows: u8) -> String {
    match (cols, rows) {
        (0, _) => tr!("Auto", "Auto").to_string(),
        (c, 0) => trf!("{c} col.", "{c} col."),
        (c, r) => format!("{c}×{r}"),
    }
}

/// Button with a small drawing of the grid shape.
pub(super) fn shape_button(
    ui: &mut egui::Ui,
    cols: u8,
    rows: u8,
    selected: bool,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(56.0, 52.0), Sense::click());
    let painter = ui.painter_at(rect);
    let bg = if selected {
        ACCENT.gamma_multiply(0.25)
    } else if resp.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(6), bg);
    if selected {
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.5, ACCENT),
            StrokeKind::Inside,
        );
    }
    let pic = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 18.0),
        Vec2::new(40.0, 24.0),
    );
    let color = if selected { ACCENT } else { weak(dark) };
    if cols == 0 {
        painter.rect_stroke(
            pic,
            CornerRadius::same(3),
            Stroke::new(1.2, color),
            StrokeKind::Inside,
        );
        painter.text(
            pic.center(),
            Align2::CENTER_CENTER,
            "A",
            FontId::proportional(13.0),
            color,
        );
    } else {
        let (c, r) = (cols as f32, rows.max(1) as f32);
        let gap = 2.0;
        let w = (pic.width() - gap * (c - 1.0)) / c;
        let h = (pic.height() - gap * (r - 1.0)) / r;
        for i in 0..cols {
            for j in 0..rows.max(1) {
                let min = pic.min + Vec2::new(i as f32 * (w + gap), j as f32 * (h + gap));
                painter.rect_filled(
                    Rect::from_min_size(min, Vec2::new(w, h)),
                    CornerRadius::same(2),
                    color,
                );
            }
        }
    }
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        shape_label(cols, rows),
        FontId::proportional(10.5),
        color,
    );
    let hint = match (cols, rows) {
        (0, _) => tr!(
            "Automático: ajusta ao tamanho da janela",
            "Automatic: fits the window size"
        ),
        (2, 1) => tr!("Lado a lado", "Side by side"),
        (1, 2) => tr!("Uma em cima da outra", "One above the other"),
        (3, 1) => tr!("Três lado a lado", "Three side by side"),
        _ => tr!("Grade fixa", "Fixed grid"),
    };
    resp.on_hover_text(hint)
}

/// Number of grid columns that gives the largest cells closest to 16:9.
pub(super) fn auto_columns(n: usize, area: Vec2, header: f32) -> usize {
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

pub(super) fn apply_style(ctx: &egui::Context, theme: Theme) {
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

pub(super) fn panel_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x18, 0x18, 0x1b)
    } else {
        Color32::from_rgb(0xf4, 0xf4, 0xf5)
    }
}

pub(super) fn content_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x0f, 0x0f, 0x12)
    } else {
        Color32::from_rgb(0xe4, 0xe4, 0xe7)
    }
}

pub(super) fn border(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2a, 0x2a, 0x30)
    } else {
        Color32::from_rgb(0xd4, 0xd4, 0xd8)
    }
}

pub(super) fn weak(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x9a, 0x9a, 0xa3)
    } else {
        Color32::from_rgb(0x63, 0x63, 0x6b)
    }
}

/// Avatar color, stable per account.
pub(super) fn avatar_color(id: &str) -> Color32 {
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

pub(super) fn avatar(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    a: &Account,
    active: bool,
) {
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
pub(super) fn single_line(
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
pub(super) fn centered(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui)) {
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
    fn automatic_columns() {
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
    fn stable_avatar_color() {
        assert_eq!(avatar_color("abc"), avatar_color("abc"));
    }
}
