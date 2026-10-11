//! Editor layout: paper background, title, two black instrument panels
//! (Engine: controls, Meter: gain reduction) and a fine-print footer that
//! explains whichever control is hovered.

use crate::{ClipMode, Meters, VepresParams};
use sacria_ui::presets::PresetBar;
use sacria_ui::{self as ui, Canvas, KnobStyle, DIM, FAINT, INK, MEDIUM, PAPER, REGULAR, WHITE};
use egui::{Align2, Vec2};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::NiceEguiApp;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Design canvas. The window scales it uniformly.
/// Footer fine print, under the panels (unshifted design coordinates).
const FOOTER_X: f32 = 28.0;
const FOOTER_Y: f32 = 442.0;
const FOOTER_W: f32 = 766.0;

/// Design canvas: height follows the footer, so the footer can never be cut off.
const DESIGN_H: f32 = FOOTER_Y + ui::FINE_HEIGHT + 12.0;
const DESIGN: Vec2 = Vec2::new(820.0, DESIGN_H);
pub const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(820.0, DESIGN_H);
pub const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(LogicalSize::new(410.0, DESIGN_H / 2.0));
const FOOTER_LABEL: &str = "VECTORS & SIGNAL";
const FOOTER_DEFAULT: &str = concat!(
    "An opinionated 3-band M/S dynamics processor. ",
    "Signal path: M/S encode > EQ > Soft clip > 3-band subtractive split > RMS detectors on a separate sidechain split ",
    "> hold + linear-dB release > makeup > M/S decode > true-peak limiter. ",
    "Hover controls for more details."
);

/// Preset bar: between the two header texts (10 pt mono, 6 units per character).
const HEAD_L: &str = "SACRIA SERIES C-VEPRES";
const HEAD_R: &str = "M/S 3-BAND DYNAMICS";
const BAR_GAP: f32 = 24.0;
const BAR_X0: f32 = 26.0 + 6.0 * HEAD_L.len() as f32 + BAR_GAP;
const BAR_X1: f32 = 794.0 - 6.0 * HEAD_R.len() as f32 - BAR_GAP;
const BAR_CY: f32 = 31.0;

const METER_RANGE_DB: f32 = 12.0;
/// Panels are drawn this far above their layout coordinates (design units).
const PANEL_SHIFT: f32 = 32.0;
/// Panel tags sit half the panel's inner padding (16 / 2) above the panel top (116).
const TAG_Y: f32 = 112.0 - 8.0;
/// Panel bottom: 16 below the row-2 value text (it ends near 448), like the panel side padding.
const PANEL_BOTTOM: f32 = 464.0;
/// Logo decal in the engine panel: center, width (design units) and opacity (0-255).
const LOGO_CX: f32 = 294.0; // 294.0 is center of the engine panel ((28 + 560) / 2)
const LOGO_CY: f32 = 230.0 * 0.6; // 230.0 is center of the engine panel ((116 + 318) / 2)
const LOGO_W: f32 = 520.0 * 1.4;
const LOGO_ALPHA: u8 = 24;
/// Meter content moves down this much so it sits centered in the taller panel.
const METER_DY: f32 = 6.0;
const KNOB_HINT: &str = ">DRAG = ADJUST, SHIFT + DRAG = FINE, DOUBLE-CLICK = DEFAULT";

/// (label, text) the footer shows per hovered control. '\n' separates paragraphs;
/// a leading '>' marks a hint paragraph (the marker is not drawn). The footer
/// joins everything into one uppercase run; the test below checks it fits.
const TIP_INPUT: (&str, &str) = (
    "01 INPUT",
    concat!(
        "Gain before the compressor. More input pushes every band harder into ",
        "compression and the limiter.\n",
    ),
);
const TIP_THRESHOLD: (&str, &str) = (
    "02 THRESHOLD",
    concat!(
        "Moves all six band thresholds (mid and side) together. Lower = more ",
        "compression, higher = less.\n",
    ),
);
const TIP_WIDTH: (&str, &str) = (
    "03 WIDTH",
    concat!(
        "Extra gain on the side bands after compression. Up = wider, down = narrower. ",
        "Mono content is not affected.\n",
    ),
);
const TIP_OUTPUT: (&str, &str) = (
    "04 OUTPUT",
    concat!(
        "Gain at the end, after the limiter. Use it to level-match when you want to ",
        "A/B against the dry signal.\n",
    ),
);
const TIP_MIX: (&str, &str) = (
    "05 MIX",
    concat!(
        "Blend between the processed and the dry signal (or parallel compression). ",
        "The dry signal is delayed to line up with the processed one, so there should be ",
        "no phasing.\n",
    ),
);
const TIP_LIMITER: (&str, &str) = (
    "06 LIMITER",
    concat!(
        "True-peak limiter at the end of the chain, threshold is set at about -1 dBTP. ",
        "Turn it off if you limit or clip later in your chain.\n>CLICK = TOGGLE",
    ),
);
const TIP_CLIP: (&str, &str) = (
    "07 CLIP",
    concat!(
        "Soft clipper before the bands. OFF = no clipping, ON = the modeled clipper, ",
        "2X = same clipper at twice the sample rate, so hot peaks do not fold back as harsh tones. ",
        "Costs a little CPU.
>CLICK = SELECT",
    ),
);
const TIP_PRESET: (&str, &str) = (
    "PRESET",
    concat!(
        "Click the name to pick a preset, or step through the list with the arrows. ",
        "The disk saves the current settings under a name you type. ",
        "A star after the name means a knob moved since the preset was loaded.",
    ),
);
const TIP_METER: (&str, &str) = (
    "GAIN REDUCTION",
    concat!(
        "How much each band is turned down right now: low, mid and high band, ",
        "for mid and side, plus the output limiter. Scale 0 to -12 dB.",
    ),
);

pub struct VepresEditor {
    params: Arc<VepresParams>,
    meters: Arc<Meters>,
    gui_ctx: Option<GuiContext>,
    logo: Option<egui::TextureHandle>,
    /// Smoothed meter values, so the ladders fall back gently.
    shown: [f32; 7],
    presets: PresetBar,
}

impl VepresEditor {
    pub fn new(params: Arc<VepresParams>, meters: Arc<Meters>) -> Self {
        let presets = PresetBar::new("C-VEPRES", params.clone(), &VepresParams::default(), params.preset.clone(), crate::FACTORY, &[]);
        Self { params, meters, gui_ctx: None, logo: None, shown: [0.0; 7], presets }
    }
}

impl NiceEguiApp for VepresEditor {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        ui::install_fonts(&egui_ctx);
        egui_ctx.set_theme(egui::Theme::Light);
        self.logo = ui::load_logo(&egui_ctx);
        self.gui_ctx = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_ctx = None;
        self.logo = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_ctx) = self.gui_ctx.as_ref() else {
            return;
        };
        let setter = gui_ctx.param_setter();
        let p = self.params.clone();

        // meters: instant attack, smooth fall
        for (s, m) in self.shown.iter_mut().zip(&self.meters.gr) {
            let v = m.load(Ordering::Relaxed);
            *s = if v < *s { v } else { *s + 0.15 * (v - *s) };
        }
        if self.shown.iter().any(|v| *v < -0.01) {
            ui.ctx().request_repaint();
        }

        let avail = ui.max_rect();
        ui.painter().rect_filled(avail, 0.0, PAPER);
        let c = Canvas::fit(avail, DESIGN);

        // ---- header ----
        for (i, line) in [HEAD_L, "ANSELIC // BY AZUR1S"].iter().enumerate() {
            c.text(ui, 26.0, 18.0 + 13.0 * i as f32, Align2::LEFT_TOP, line, 10.0, MEDIUM, INK);
        }
        for (i, line) in [HEAD_R, concat!("VER ", env!("CARGO_PKG_VERSION"))].iter().enumerate() {
            c.text(ui, 794.0, 18.0 + 13.0 * i as f32, Align2::RIGHT_TOP, line, 10.0, MEDIUM, INK);
        }

        // panels sit closer to the header: draw them on a canvas shifted up
        let b = c.offset(0.0, -PANEL_SHIFT);

        // ---- panel: Engine ----
        ui::tag(ui, &b, 28.0, TAG_Y, "engine");
        let engine_rect = b.r(28.0, 116.0, 560.0, PANEL_BOTTOM);
        ui.painter().rect_filled(engine_rect, b.l(8.0), INK);
        // logo decal behind the knobs, clipped to the panel
        if let Some(logo) = &self.logo {
            let (w, h) = (LOGO_W, LOGO_W / ui::LOGO_ASPECT);
            let rect = b.r(LOGO_CX - w / 2.0, LOGO_CY - h / 2.0, LOGO_CX + w / 2.0, LOGO_CY + h / 2.0);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .with_clip_rect(engine_rect)
                .image(logo.id(), rect, uv, egui::Color32::from_white_alpha(LOGO_ALPHA));
        }
        b.line(ui, &[(44.0, 134.0), (544.0, 134.0)], 2.0, WHITE);
        b.text(ui, 44.0, 140.0, Align2::LEFT_TOP, "DYNAMICS CONTROL", 8.0, REGULAR, DIM);
        b.text(ui, 544.0, 140.0, Align2::RIGHT_TOP, "MODEL // ANSLC", 8.0, REGULAR, DIM);

        let big_bi = || KnobStyle { radius: 36.0, bipolar: true };
        let small = || KnobStyle { radius: 28.0, bipolar: false };
        // which control the footer explains: (title, body), is a knob
        let mut hovered: Option<((&str, &str), bool)> = None;
        let mut track = |resp: egui::Response, tip: (&'static str, &'static str), is_knob: bool| {
            if resp.hovered() || resp.dragged() {
                hovered = Some((tip, is_knob));
            }
        };
        track(self.presets.show(ui, &c, BAR_X0, BAR_X1, BAR_CY, gui_ctx), TIP_PRESET, false);
        track(ui::knob(ui, &b, 117.0, 230.0, "01", "INPUT", &p.input, &setter, big_bi()), TIP_INPUT, true);
        track(ui::knob(ui, &b, 294.0, 230.0, "02", "THRESHOLD", &p.threshold, &setter, big_bi()), TIP_THRESHOLD, true);
        track(ui::knob(ui, &b, 471.0, 230.0, "03", "WIDTH", &p.width, &setter, big_bi()), TIP_WIDTH, true);
        // divider between rows
        b.line(ui, &[(44.0, 318.0), (544.0, 318.0)], 1.0, FAINT);
        let out_style = KnobStyle { bipolar: true, ..small() };
        track(ui::knob(ui, &b, 117.0, 384.0, "04", "OUTPUT", &p.output, &setter, out_style), TIP_OUTPUT, true);
        track(ui::knob(ui, &b, 294.0, 384.0, "05", "MIX", &p.mix, &setter, small()), TIP_MIX, true);
        // switch labels line up with the knob labels of row 2 (cy - 24 = 384 - 28 - 30)
        let on = p.limiter.value();
        let sw = ui::switch(ui, &b, 471.0, 350.0, "06", "LIMITER", on);
        if sw.clicked() {
            setter.begin_set_parameter(&p.limiter);
            setter.set_parameter(&p.limiter, !on);
            setter.end_set_parameter(&p.limiter);
        }
        track(sw, TIP_LIMITER, false);
        let modes = [ClipMode::Off, ClipMode::On, ClipMode::X2];
        let cur = modes.iter().position(|m| *m == p.clip.value()).unwrap_or(1);
        let (sw, picked) = ui::switch3(ui, &b, 471.0, 420.0, "07", "CLIP", ["OFF", "ON", "2X"], cur);
        if let Some(i) = picked.filter(|i| *i != cur) {
            setter.begin_set_parameter(&p.clip);
            setter.set_parameter(&p.clip, modes[i]);
            setter.end_set_parameter(&p.clip);
        }
        track(sw, TIP_CLIP, false);

        // ---- panel: Meter ----
        ui::tag(ui, &b, 576.0, TAG_Y, "meter");
        ui.painter().rect_filled(b.r(576.0, 116.0, 794.0, PANEL_BOTTOM), b.l(8.0), INK);
        b.line(ui, &[(590.0, 134.0), (780.0, 134.0)], 2.0, WHITE);
        b.text(ui, 590.0, 140.0, Align2::LEFT_TOP, "GAIN REDUCTION", 8.0, REGULAR, DIM);
        let (y0, y1) = (164.0 + METER_DY, 396.0 + METER_DY);
        // dB scale
        for i in 0..=4 {
            let db = -3.0 * i as f32;
            let y = y0 + (y1 - y0) * (-db / METER_RANGE_DB);
            b.text(ui, 606.0, y, Align2::RIGHT_CENTER, &format!("{db:.0}"), 7.5, REGULAR, DIM);
            b.line(ui, &[(609.0, y), (613.0, y)], 1.0, DIM);
        }
        let labels = ["LO", "MI", "HI", "LO", "MI", "HI", "LIM"];
        for (k, v) in self.shown.iter().enumerate() {
            let x = 618.0 + 22.0 * k as f32 + if k >= 3 { 6.0 } else { 0.0 } + if k == 6 { 6.0 } else { 0.0 };
            ui::ladder(ui, &b, x, y0, y1, 14.0, *v, METER_RANGE_DB);
            b.text(ui, x + 7.0, y1 + 8.0, Align2::CENTER_TOP, labels[k], 7.5, REGULAR, if *v < -0.1 { WHITE } else { DIM });
        }
        b.line(ui, &[(618.0, 424.0 + METER_DY), (680.0, 424.0 + METER_DY)], 1.0, DIM);
        b.text(ui, 649.0, 428.0 + METER_DY, Align2::CENTER_TOP, "MID", 8.0, MEDIUM, WHITE);
        b.line(ui, &[(690.0, 424.0 + METER_DY), (752.0, 424.0 + METER_DY)], 1.0, DIM);
        b.text(ui, 721.0, 428.0 + METER_DY, Align2::CENTER_TOP, "SIDE", 8.0, MEDIUM, WHITE);

        let meter_area = ui.interact(b.r(576.0, 116.0, 794.0, PANEL_BOTTOM), ui.id().with("meter-panel"), egui::Sense::hover());
        if meter_area.hovered() {
            hovered = Some((TIP_METER, false));
        }

        // ---- footer: explains the hovered control, else the default fine print ----
        let (label, text) = match hovered {
            Some(((title, body), true)) => (title, format!("{body}{KNOB_HINT}")),
            Some(((title, body), false)) => (title, body.to_owned()),
            None => (FOOTER_LABEL, FOOTER_DEFAULT.to_owned()),
        };
        ui::fine_print(ui, &c, FOOTER_X, FOOTER_Y, FOOTER_W, label, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_footer_text_fits() {
        let mut all = vec![(FOOTER_LABEL, FOOTER_DEFAULT.to_owned())];
        for (tip, knob) in [
            (TIP_INPUT, true),
            (TIP_THRESHOLD, true),
            (TIP_WIDTH, true),
            (TIP_OUTPUT, true),
            (TIP_MIX, true),
            (TIP_LIMITER, false),
            (TIP_CLIP, false),
            (TIP_METER, false),
            (TIP_PRESET, false),
        ] {
            all.push((tip.0, if knob { format!("{}{KNOB_HINT}", tip.1) } else { tip.1.to_owned() }));
        }
        for (label, text) in all {
            let n = ui::fine_print_lines(FOOTER_W, &text).len();
            assert!(n <= ui::FINE_LINES, "{label}: needs {n} lines, footer has {}", ui::FINE_LINES);
        }
    }
}
