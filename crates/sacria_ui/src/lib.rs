//! Shared UI kit for the Sacria plugins.
//!
//! Hand-painted widgets: instrument-panel style (black panels, thin white line art,
//! mono type). Everything is drawn on a fixed design canvas that scales with the window.

use egui::{epaint::PathStroke, Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use nice_plug::prelude::{Param, ParamSetter};
use std::f32::consts::PI;
use std::sync::Arc;

pub mod presets;

pub const PAPER: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd2);
pub const INK: Color32 = Color32::from_rgb(0x0e, 0x0e, 0x0e);
pub const WHITE: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xee);
pub const DIM: Color32 = Color32::from_rgb(0x6a, 0x6a, 0x66);
pub const FAINT: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2a);

pub const REGULAR: &str = "geist-regular";
pub const MEDIUM: &str = "geist-medium";
pub const BOLD: &str = "geist-bold";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (REGULAR, &include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf")[..]),
        (MEDIUM, &include_bytes!("../../../assets/fonts/GeistMono-Medium.ttf")[..]),
        (BOLD, &include_bytes!("../../../assets/fonts/GeistMono-Bold.ttf")[..]),
    ] {
        fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_static(bytes)));
        fonts.families.insert(FontFamily::Name(name.into()), vec![name.to_owned()]);
    }
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.get_mut(&fam).unwrap().insert(0, REGULAR.to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Maps design coordinates (fixed canvas) to screen coordinates.
#[derive(Clone, Copy)]
pub struct Canvas {
    pub origin: Pos2,
    pub s: f32,
}

impl Canvas {
    /// Fits a `size` design canvas into `avail`, uniformly scaled and centered.
    pub fn fit(avail: Rect, size: Vec2) -> Self {
        let s = (avail.width() / size.x).min(avail.height() / size.y);
        let origin = avail.center() - size * s * 0.5;
        Self { origin, s }
    }
    /// Same scale, origin moved by (dx, dy) design units.
    pub fn offset(&self, dx: f32, dy: f32) -> Self {
        Self { origin: self.origin + Vec2::new(dx, dy) * self.s, s: self.s }
    }
    pub fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.s
    }
    pub fn r(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
    pub fn l(&self, v: f32) -> f32 {
        v * self.s
    }
    pub fn font(&self, size: f32, family: &str) -> FontId {
        FontId::new(size * self.s, FontFamily::Name(family.into()))
    }
    pub fn text(&self, ui: &Ui, x: f32, y: f32, align: Align2, text: &str, size: f32, family: &str, color: Color32) {
        ui.painter().text(self.p(x, y), align, text, self.font(size, family), color);
    }
    pub fn line(&self, ui: &Ui, pts: &[(f32, f32)], width: f32, color: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        ui.painter().add(Shape::line(pts, Stroke::new(self.l(width), color)));
    }
}

/// The logo decal (white on transparent). Width / height of the PNG.
pub const LOGO_ASPECT: f32 = 596.0 / 226.0;

/// Decodes the embedded logo PNG into a texture. None if decoding fails.
pub fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes: &[u8] = include_bytes!("../../../assets/azur1s_logo_white.png");
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let px = &buf[..info.buffer_size()];
    let img = match info.color_type {
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied(size, px),
        png::ColorType::GrayscaleAlpha => {
            let rgba: Vec<u8> = px.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect();
            egui::ColorImage::from_rgba_unmultiplied(size, &rgba)
        }
        _ => return None,
    };
    let opts = egui::TextureOptions {
        mipmap_mode: Some(egui::TextureFilter::Linear),
        ..egui::TextureOptions::LINEAR
    };
    Some(ctx.load_texture("logo", img, opts))
}

/// Poster-style bold tag above a panel.
pub fn tag(ui: &Ui, c: &Canvas, x: f32, y: f32, text: &str) {
    c.text(ui, x, y + 3.0, Align2::LEFT_BOTTOM, text, 17.0, MEDIUM, INK);
}

/// Font sizes of the fine print and the lines it may use.
const FINE_SIZE: f32 = 9.0;
const FINE_LABEL_SIZE: f32 = 10.0;
const FINE_LINE_H: f32 = FINE_SIZE * 1.4;
pub const FINE_LINES: usize = 3;
/// Total height `fine_print` uses (design units).
pub const FINE_HEIGHT: f32 = FINE_LABEL_SIZE + 6.0 + FINE_LINE_H * FINE_LINES as f32;

/// Poster-style fine print: black block + bold label, then small uppercase text
/// wrapped over up to `FINE_LINES` lines of `width` design units.
pub fn fine_print(ui: &Ui, c: &Canvas, x: f32, y: f32, width: f32, label: &str, text: &str) {
    let mid = y + FINE_LABEL_SIZE / 2.0;
    ui.painter().rect_filled(c.r(x, y + 1.0, x + 44.0, y + FINE_LABEL_SIZE - 1.0), 0.0, INK);
    c.text(ui, x + 50.0, mid, Align2::LEFT_CENTER, label, FINE_LABEL_SIZE, BOLD, INK);
    let top = y + FINE_LABEL_SIZE + 6.0;
    for (i, line) in fine_print_lines(width, text).iter().take(FINE_LINES).enumerate() {
        c.text(ui, x, top + FINE_LINE_H * i as f32, Align2::LEFT_TOP, line, FINE_SIZE, REGULAR, INK);
    }
}

/// The wrapped, uppercase lines `fine_print` draws (before the `FINE_LINES` cut).
pub fn fine_print_lines(width: f32, text: &str) -> Vec<String> {
    // mono: every glyph is ~0.6 em wide
    let chars = (width / (FINE_SIZE * 0.6)) as usize;
    // one running paragraph; drop only the '>' hint marker at a paragraph start
    let flat: Vec<&str> = text.split('\n').map(|p| p.trim_start_matches('>')).filter(|p| !p.is_empty()).collect();
    wrap(&flat.join(" ").to_uppercase(), chars)
}

/// Word wrap at `width` characters (mono font).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push(line);
    out
}

fn arc_points(c: &Canvas, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = ((a1 - a0).abs() * r / 3.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c.p(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Text above a control. A `code` that starts with '#' only makes the widget id
/// unique (same label in several panels) and is not drawn.
fn caption(code: &str, label: &str) -> String {
    if code.starts_with('#') {
        label.to_owned()
    } else {
        format!("{code} {label}")
    }
}

/// Knob angle for a normalized value: 270 degree sweep, gap at the bottom.
fn knob_angle(t: f32) -> f32 {
    PI * (0.75 + 1.5 * t)
}

pub struct KnobStyle {
    pub radius: f32,
    /// Value arc starts at the middle (for +/- offsets).
    pub bipolar: bool,
}

/// A parameter knob. Drag up/down (Shift = fine), double-click = default.
/// `code` + `label` must be unique in the window (they form the widget id).
/// Start `code` with '#' to keep it out of the caption.
pub fn knob<P: Param>(
    ui: &mut Ui,
    c: &Canvas,
    cx: f32,
    cy: f32,
    code: &str,
    label: &str,
    param: &P,
    setter: &ParamSetter,
    style: KnobStyle,
) -> Response {
    let r = style.radius;
    let hit = Rect::from_center_size(c.p(cx, cy), Vec2::splat(c.l(2.0 * r + 24.0)));
    let id = ui.id().with(("knob", code, label));
    let resp: Response = ui.interact(hit, id, Sense::click_and_drag());

    if resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    } else {
        if resp.drag_started() {
            setter.begin_set_parameter(param);
            ui.memory_mut(|m| m.data.insert_temp(id, param.unmodulated_normalized_value()));
        }
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let per_px = if fine { 0.0007 } else { 0.005 };
            let mut v: f32 = ui.memory(|m| m.data.get_temp(id)).unwrap_or(param.unmodulated_normalized_value());
            v = (v - resp.drag_delta().y / c.s * per_px).clamp(0.0, 1.0);
            ui.memory_mut(|m| m.data.insert_temp(id, v));
            setter.set_parameter_normalized(param, v);
        }
        if resp.drag_stopped() {
            setter.end_set_parameter(param);
        }
    }

    let t = param.unmodulated_normalized_value();
    let active = resp.hovered() || resp.dragged();
    let p = ui.painter();

    // scale: 31 ticks, every 5th long
    for i in 0..=30 {
        let a = knob_angle(i as f32 / 30.0);
        let (r0, r1, w) = if i % 5 == 0 { (r + 6.0, r + 13.0, 1.4) } else { (r + 6.0, r + 9.0, 1.0) };
        p.line_segment(
            [c.p(cx + r0 * a.cos(), cy + r0 * a.sin()), c.p(cx + r1 * a.cos(), cy + r1 * a.sin())],
            Stroke::new(c.l(w), if i % 5 == 0 { WHITE } else { DIM }),
        );
    }
    // value arc
    let from: f32 = if style.bipolar { 0.5 } else { 0.0 };
    let (a0, a1) = (knob_angle(from.min(t)), knob_angle(from.max(t)));
    if (a1 - a0).abs() > 1e-3 {
        p.add(Shape::line(arc_points(c, cx, cy, r + 3.0, a0, a1), PathStroke::new(c.l(2.2), WHITE)));
    }
    // body
    p.circle(c.p(cx, cy), c.l(r), INK, Stroke::new(c.l(1.4), WHITE));
    p.circle_stroke(c.p(cx, cy), c.l(r * 0.62), Stroke::new(c.l(1.0), if active { DIM } else { FAINT }));
    let a = knob_angle(t);
    p.line_segment(
        [c.p(cx + 0.25 * r * a.cos(), cy + 0.25 * r * a.sin()), c.p(cx + 0.92 * r * a.cos(), cy + 0.92 * r * a.sin())],
        Stroke::new(c.l(2.4), WHITE),
    );

    // label over, value under
    c.text(ui, cx, cy - r - 30.0, Align2::CENTER_BOTTOM, &caption(code, label), 10.0, MEDIUM, WHITE);
    let value = param.normalized_value_to_string(t, true).to_uppercase();
    c.text(ui, cx, cy + r + 24.0, Align2::CENTER_TOP, &value, 10.0, REGULAR, if active { WHITE } else { DIM });
    resp
}

/// OFF/ON pill switch, label above. Check `.clicked()` on the result.
pub fn switch(ui: &mut Ui, c: &Canvas, cx: f32, cy: f32, code: &str, label: &str, on: bool) -> Response {
    let (w, h) = (46.0, 20.0);
    let rect = c.r(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
    let resp = ui.interact(rect.expand(c.l(8.0)), ui.id().with(("switch", code, label)), Sense::click());
    let p = ui.painter();
    p.rect_stroke(rect, c.l(h / 2.0), Stroke::new(c.l(1.4), WHITE), egui::StrokeKind::Inside);
    let kx = if on { cx + w / 2.0 - h / 2.0 } else { cx - w / 2.0 + h / 2.0 };
    p.circle_filled(c.p(kx, cy), c.l(h / 2.0 - 4.0), WHITE);
    c.text(ui, cx - w / 2.0 - 8.0, cy, Align2::RIGHT_CENTER, "OFF", 9.0, REGULAR, if on { DIM } else { WHITE });
    c.text(ui, cx + w / 2.0 + 8.0, cy, Align2::LEFT_CENTER, "ON", 9.0, REGULAR, if on { WHITE } else { DIM });
    c.text(ui, cx, cy - 24.0, Align2::CENTER_BOTTOM, &caption(code, label), 10.0, MEDIUM, WHITE);
    resp
}

/// Three-way segmented switch, label above. Returns the hover response of the
/// whole switch and the segment that was clicked, if any.
pub fn switch3(
    ui: &mut Ui,
    c: &Canvas,
    cx: f32,
    cy: f32,
    code: &str,
    label: &str,
    names: [&str; 3],
    selected: usize,
) -> (Response, Option<usize>) {
    let (seg, h) = (32.0, 20.0);
    let x0 = cx - 1.5 * seg;
    let rect = c.r(x0, cy - h / 2.0, x0 + 3.0 * seg, cy + h / 2.0);
    let mut resp = ui.interact(rect.expand(c.l(8.0)), ui.id().with(("switch3", code, label)), Sense::hover());
    let mut clicked = None;
    for (i, name) in names.iter().enumerate() {
        let r = c.r(x0 + seg * i as f32, cy - h / 2.0, x0 + seg * (i + 1) as f32, cy + h / 2.0);
        let r_hit = if i == 0 { r.expand2(Vec2::new(c.l(8.0), c.l(8.0))) } else { r.expand2(Vec2::new(0.0, c.l(8.0))) };
        let seg_resp = ui.interact(r_hit, ui.id().with(("switch3", code, label, i)), Sense::click());
        if seg_resp.clicked() {
            clicked = Some(i);
        }
        resp = resp.union(seg_resp);
        let on = i == selected;
        if on {
            ui.painter().rect_filled(r.shrink(c.l(3.0)), c.l(h / 2.0 - 3.0), WHITE);
        }
        c.text(ui, x0 + seg * (i as f32 + 0.5), cy, Align2::CENTER_CENTER, name, 9.0, REGULAR, if on { INK } else { DIM });
    }
    ui.painter().rect_stroke(rect, c.l(h / 2.0), Stroke::new(c.l(1.4), WHITE), egui::StrokeKind::Inside);
    c.text(ui, cx, cy - 24.0, Align2::CENTER_BOTTOM, &caption(code, label), 10.0, MEDIUM, WHITE);
    (resp, clicked)
}

/// Segmented switch without a label: `names.len()` segments of `seg_w` design
/// units, centred on (cx, cy). `on_paper` picks the colors for the paper background. `id` must be unique in the window. Returns the
/// hover response of the whole switch and the segment that was clicked, if any.
pub fn segmented(
    ui: &mut Ui,
    c: &Canvas,
    cx: f32,
    cy: f32,
    id: &str,
    names: &[&str],
    seg_w: f32,
    selected: usize,
    on_paper: bool,
) -> (Response, Option<usize>) {
    // on a black panel: white pill, ink text. On the paper background: the other way round.
    let (line, picked_text) = if on_paper { (INK, PAPER) } else { (WHITE, INK) };
    let h = 20.0;
    let x0 = cx - 0.5 * seg_w * names.len() as f32;
    let rect = c.r(x0, cy - h / 2.0, x0 + seg_w * names.len() as f32, cy + h / 2.0);
    let mut resp = ui.interact(rect.expand(c.l(6.0)), ui.id().with(("segmented", id)), Sense::hover());
    let mut clicked = None;
    for (i, name) in names.iter().enumerate() {
        let r = c.r(x0 + seg_w * i as f32, cy - h / 2.0, x0 + seg_w * (i + 1) as f32, cy + h / 2.0);
        let seg_resp = ui.interact(r.expand2(Vec2::new(0.0, c.l(6.0))), ui.id().with(("segmented", id, i)), Sense::click());
        if seg_resp.clicked() {
            clicked = Some(i);
        }
        resp = resp.union(seg_resp);
        let on = i == selected;
        if on {
            ui.painter().rect_filled(r.shrink(c.l(3.0)), c.l(h / 2.0 - 3.0), line);
        }
        c.text(ui, x0 + seg_w * (i as f32 + 0.5), cy, Align2::CENTER_CENTER, name, 9.0, REGULAR, if on { picked_text } else { DIM });
    }
    ui.painter().rect_stroke(rect, c.l(h / 2.0), Stroke::new(c.l(1.4), line), egui::StrokeKind::Inside);
    (resp, clicked)
}

/// Segmented gain-reduction ladder, 0 dB at the top.
pub fn ladder(ui: &Ui, c: &Canvas, x: f32, y0: f32, y1: f32, w: f32, gr_db: f32, range_db: f32) {
    let n = 24;
    let seg = (y1 - y0) / n as f32;
    let lit = ((-gr_db / range_db) * n as f32).clamp(0.0, n as f32);
    for i in 0..n {
        let r = c.r(x, y0 + seg * i as f32 + 1.0, x + w, y0 + seg * (i + 1) as f32 - 1.0);
        if (i as f32) < lit.floor() || ((i as f32) < lit && lit.fract() > 0.5) {
            ui.painter().rect_filled(r, 0.0, WHITE);
        } else {
            ui.painter().rect_stroke(r, 0.0, Stroke::new(c.l(0.8), FAINT), egui::StrokeKind::Inside);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_decodes() {
        let logo = load_logo(&egui::Context::default()).expect("logo PNG failed to decode");
        let [w, h] = logo.size();
        assert!((w as f32 / h as f32 - LOGO_ASPECT).abs() < 0.01, "{w}x{h}");
    }
}
