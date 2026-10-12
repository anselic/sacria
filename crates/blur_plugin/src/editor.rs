//! One black panel: the engine's controls in a row on the left, MIX and OUT on the right.
//! The fine-print footer explains whichever control is hovered.

use crate::BlurParams;
use egui::Align2;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::NiceEguiApp;
use sacria_ui::presets::PresetBar;
use sacria_ui::{self as ui, Canvas, KnobStyle, FAINT, INK, MEDIUM, PAPER};
use std::sync::Arc;

const HEAD_L: &str = "SACRIA SERIES S-BLUR";
const HEAD_R: &str = "PER-PARTIAL ATTACK AND DECAY";

const COL_W: f32 = 112.0;
const SIDE: f32 = 84.0;
const GAP: f32 = 36.0;
const PANEL_H: f32 = 268.0;
const KNOB_Y: f32 = 168.0;
const KNOB_R: f32 = 24.0;
/// The footer: 10 above the text, 12 under (the panel's own 20 reused).
const DESIGN_H: f32 = PANEL_H + ui::FINE_HEIGHT + 2.0;
const DESIGN_W: f32 = 2.0 * SIDE + (CONTROLS.len() as f32 + 1.0) * COL_W + GAP;

/// Preset bar: between the two header texts (10 pt mono, 6 units per character).
const BAR_GAP: f32 = 24.0;
const BAR_X0: f32 = 26.0 + 6.0 * HEAD_L.len() as f32 + BAR_GAP;
const BAR_X1: f32 = DESIGN_W - 26.0 - 6.0 * HEAD_R.len() as f32 - BAR_GAP;
const BAR_CY: f32 = 31.0;

pub const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(DESIGN_W, DESIGN_H);
pub const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(LogicalSize::new(DESIGN_W / 2.0, DESIGN_H / 2.0));

const ABOUT: &str = concat!(
    "Every partial of the sound gets its own attack and decay time, so the tone smears in time ",
    "without any room sound. Signal path: spectrum > per-partial attack and decay > ringing partials ",
    "keep their own pitch > source cut. Hover controls for more details."
);
const KNOB_HINT: &str = ">DRAG = ADJUST, SHIFT + DRAG = FINE, DOUBLE-CLICK = DEFAULT";
const TIP_PRESET: &str = concat!(
    "Click the name to pick a preset, or step through the list with the arrows. ",
    "The disk saves the current settings under a name you type. ",
    "A star after the name means a knob moved since the preset was loaded.",
);
const TIP_MIX: &str = concat!(
    "Blend between the processed and the dry signal. The dry signal is delayed to line up with ",
    "the processed one, so there should be no phasing.\n",
);
const TIP_OUT: &str = "Gain at the end. Use it to level-match when you want to A/B against the dry signal.\n";

/// The engine's controls, left to right: name, footer text, is a knob.
const CONTROLS: &[(&str, &str, bool)] = &[
    ("ATTACK", "How slowly each partial fades in. 0 = hits stay sharp. Longer = hits get soft and the sound swells in.\n", true),
    ("DECAY", "How long each partial rings after the input lets go of it (time to fall 60 dB). Longer = a longer tail.\n", true),
    (
        "SCATTER",
        "Random phase on the ringing partials. Low = the tail is a clean, static tone. High = the tail turns to noise and wash.\n",
        true,
    ),
    (
        "SOURCE",
        concat!(
            "How much of the input stays in the output. 100% = the normal effect. 0% = the input is taken out, ",
            "so only what the blur added or removed is left (a null test). Keep MIX at 100% for the pure null sound.\n",
        ),
        true,
    ),
    ("FREEZE", "Holds the tone as it is right now and ignores the input until it is turned off.\n>CLICK = TOGGLE", false),
];

/// Draws the controls of `CONTROLS` one after the other and remembers which one is hovered.
struct Row<'a> {
    c: &'a Canvas,
    setter: &'a ParamSetter<'a>,
    at: usize,
    /// What the footer explains: title, body, is a knob.
    hovered: Option<(String, &'static str, bool)>,
}

impl Row<'_> {
    fn track(&mut self, resp: egui::Response, title: String, tip: &'static str, is_knob: bool) {
        if resp.hovered() || resp.dragged() {
            self.hovered = Some((title, tip, is_knob));
        }
    }

    fn knob<P: Param>(&mut self, ui: &mut egui::Ui, param: &P) {
        let (name, tip, _) = CONTROLS[self.at];
        let code = format!("{:02}", self.at + 1);
        let x = SIDE + COL_W * self.at as f32;
        let resp = ui::knob(ui, self.c, x, KNOB_Y, &code, name, param, self.setter, KnobStyle { radius: KNOB_R, bipolar: false });
        self.track(resp, format!("{code} {name}"), tip, true);
        self.at += 1;
    }

    fn switch(&mut self, ui: &mut egui::Ui, param: &BoolParam) {
        let (name, tip, _) = CONTROLS[self.at];
        let code = format!("{:02}", self.at + 1);
        let on = param.value();
        let resp = ui::switch(ui, self.c, SIDE + COL_W * self.at as f32, KNOB_Y, &code, name, on);
        if resp.clicked() {
            self.setter.begin_set_parameter(param);
            self.setter.set_parameter(param, !on);
            self.setter.end_set_parameter(param);
        }
        self.track(resp, format!("{code} {name}"), tip, false);
        self.at += 1;
    }
}

pub struct BlurEditor {
    params: Arc<BlurParams>,
    gui_ctx: Option<GuiContext>,
    presets: PresetBar,
}

impl BlurEditor {
    pub fn new(params: Arc<BlurParams>) -> Self {
        let presets = PresetBar::new("S-BLUR", params.clone(), &BlurParams::default(), params.preset.clone(), crate::FACTORY, &[]);
        Self { params, gui_ctx: None, presets }
    }
}

impl NiceEguiApp for BlurEditor {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        ui::install_fonts(&egui_ctx);
        egui_ctx.set_theme(egui::Theme::Light);
        self.gui_ctx = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_ctx = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_ctx) = self.gui_ctx.as_ref() else {
            return;
        };
        let setter = gui_ctx.param_setter();
        let p = self.params.clone();

        let avail = ui.max_rect();
        ui.painter().rect_filled(avail, 0.0, PAPER);
        let c = Canvas::fit(avail, egui::vec2(DESIGN_W, DESIGN_H));

        for (i, line) in [HEAD_L, "ANSELIC // BY AZUR1S"].iter().enumerate() {
            c.text(ui, 26.0, 18.0 + 13.0 * i as f32, Align2::LEFT_TOP, line, 10.0, MEDIUM, INK);
        }
        for (i, line) in [HEAD_R, concat!("VER ", env!("CARGO_PKG_VERSION"))].iter().enumerate() {
            c.text(ui, DESIGN_W - 26.0, 18.0 + 13.0 * i as f32, Align2::RIGHT_TOP, line, 10.0, MEDIUM, INK);
        }
        let bottom = PANEL_H - 20.0;
        ui.painter().rect_filled(c.r(28.0, 60.0, DESIGN_W - 28.0, bottom), c.l(8.0), INK);
        c.line(ui, &[(44.0, 84.0), (DESIGN_W - 44.0, 84.0)], 1.0, FAINT);

        let bar = self.presets.show(ui, &c, BAR_X0, BAR_X1, BAR_CY, gui_ctx);
        let mut row = Row { c: &c, setter: &setter, at: 0, hovered: None };
        row.track(bar, "PRESET".to_owned(), TIP_PRESET, false);
        row.knob(ui, &p.attack);
        row.knob(ui, &p.decay);
        row.knob(ui, &p.scatter);
        row.knob(ui, &p.source);
        row.switch(ui, &p.freeze);

        let x0 = SIDE + COL_W * CONTROLS.len() as f32 + GAP;
        c.line(ui, &[(x0 - COL_W / 2.0 - GAP / 2.0, 100.0), (x0 - COL_W / 2.0 - GAP / 2.0, bottom - 16.0)], 1.0, FAINT);
        let mix = ui::knob(ui, &c, x0, KNOB_Y, "#mix", "MIX", &p.mix, &setter, KnobStyle { radius: KNOB_R, bipolar: false });
        row.track(mix, "MIX".to_owned(), TIP_MIX, true);
        let out = ui::knob(ui, &c, x0 + COL_W, KNOB_Y, "#out", "OUT", &p.output, &setter, KnobStyle { radius: KNOB_R, bipolar: false });
        row.track(out, "OUT".to_owned(), TIP_OUT, true);

        // footer: explains the hovered control, else what the plugin is
        let (label, text) = match row.hovered {
            Some((title, body, true)) => (title, format!("{body}{KNOB_HINT}")),
            Some((title, body, false)) => (title, body.to_owned()),
            None => ("S-BLUR".to_owned(), ABOUT.to_owned()),
        };
        ui::fine_print(ui, &c, 28.0, bottom + 10.0, DESIGN_W - 56.0, &label, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_footer_text_fits() {
        let mut all = vec![("S-BLUR", ABOUT.to_owned()), ("PRESET", TIP_PRESET.to_owned())];
        for tip in [TIP_MIX, TIP_OUT] {
            all.push(("MIX / OUT", format!("{tip}{KNOB_HINT}")));
        }
        for &(name, tip, is_knob) in CONTROLS {
            all.push((name, if is_knob { format!("{tip}{KNOB_HINT}") } else { tip.to_owned() }));
        }
        for (label, text) in all {
            let n = ui::fine_print_lines(DESIGN_W - 56.0, &text).len();
            assert!(n <= ui::FINE_LINES, "{label}: needs {n} lines, footer has {}", ui::FINE_LINES);
        }
    }
}
