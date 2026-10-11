//! Editor layout: paper background, title, a response strip (what the band
//! gains do to mid and side), one black instrument panel per band with a live
//! transfer-curve scope, an output panel, and a fine-print footer that explains
//! whichever control is hovered.

use crate::spectrum::{self, Analyzer};
use crate::wave;
use crate::{BandKnobs, DetectMode, Meters, Quality, SanctisParams, ShapeMode, StageMode};
use egui::{Align2, Color32, Vec2};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::NiceEguiApp;
use sacria_dsp::util::gain_reduction_db;
use sacria_ui::presets::PresetBar;
use sacria_ui::{self as ui, Canvas, KnobStyle, DIM, FAINT, INK, MEDIUM, PAPER, REGULAR, WHITE};
use std::sync::atomic::Ordering;
use std::sync::Arc;

// ---- layout (design units) ----
const DESIGN_W: f32 = 1276.0;
const MARGIN: f32 = 28.0;
/// Preset bar: between the two header texts (10 pt mono, 6 units per character).
const HEAD_L: &str = "SACRIA SERIES C-SANCTIS";
const HEAD_R: &str = "3-BAND MASTERING GLUE";
const BAR_GAP: f32 = 24.0;
const BAR_X0: f32 = MARGIN - 2.0 + 6.0 * HEAD_L.len() as f32 + BAR_GAP;
const BAR_X1: f32 = DESIGN_W - MARGIN - 6.0 * HEAD_R.len() as f32 - BAR_GAP;
const BAR_CY: f32 = 31.0;
/// Response strip.
const STRIP_Y0: f32 = 64.0;
const STRIP_Y1: f32 = 176.0;
/// Panels: four of the same width (low, mid, high, output).
const PANEL_W: f32 = 296.0;
const PANEL_GAP: f32 = 12.0;
const PANEL_Y0: f32 = 212.0;
const PANEL_Y1: f32 = PANEL_Y0 + 396.0;
const TAG_Y: f32 = PANEL_Y0 - 12.0;
/// Scope / meter area at the top of each panel.
const SCOPE_Y0: f32 = PANEL_Y0 + 44.0;
const SCOPE_Y1: f32 = PANEL_Y0 + 224.0;
/// One knob row per panel; the page switch picks which four knobs it shows.
const ROW: f32 = PANEL_Y0 + 324.0;
/// MODE page: switch pills, so that caption + pill sit centred between the divider and the panel bottom.
/// (caption top is 24 above the pill centre, pill bottom 10 below it: 34 in all)
const MODE_MID: f32 = (SCOPE_Y1 + 22.0 + PANEL_Y1) / 2.0;
const MODE_Y: f32 = MODE_MID + 7.0;
/// Two switches stacked: 34 each with 28 of air between them, centred on MODE_MID.
const MODE_Y_TOP: f32 = MODE_MID - 48.0 + 24.0;
const MODE_Y_BOTTOM: f32 = MODE_Y_TOP + 62.0;
const KNOB_R: f32 = 22.0;
const KNOB_DX: f32 = 72.0;
const KNOB_X0: f32 = 40.0;

const FOOTER_X: f32 = MARGIN;
const FOOTER_Y: f32 = PANEL_Y1 + 18.0;
const FOOTER_W: f32 = DESIGN_W - 2.0 * MARGIN;
/// Design canvas: height follows the footer, so the footer can never be cut off.
const DESIGN_H: f32 = FOOTER_Y + ui::FINE_HEIGHT + 12.0;
const DESIGN: Vec2 = Vec2::new(DESIGN_W, DESIGN_H);
pub const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(DESIGN_W, DESIGN_H);
pub const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(LogicalSize::new(DESIGN_W / 2.0, DESIGN_H / 2.0));

/// Band split corners of the engine (fixed), for the response strip.
const SPLIT_LO_HZ: f32 = 161.26;
const SPLIT_HI_HZ: f32 = 2509.4;
const STRIP_DB: f32 = 9.0;
/// Output spectrum in the strip: dB at the top and bottom edge of the plot (a full-scale sine = 0 dB).
const SPEC_TOP_DB: f32 = -18.0;
const SPEC_BOTTOM_DB: f32 = -78.0;
/// Scope range: detector level in, band level out (dBFS).
const SCOPE_MIN: f32 = -48.0;
const SCOPE_MAX: f32 = 6.0;
const METER_RANGE_DB: f32 = 12.0;
/// Logo decal in the response strip (design units). It is clipped to the strip.
/// LOGO_CX: centre, left to right (strip runs from 28 to 1248). Bigger = further right.
/// LOGO_CY: centre, top to bottom (strip runs from 64 to 176). Bigger = further down.
/// LOGO_W: width; the height follows the image.
// const LOGO_CX: f32 = DESIGN_W - MARGIN - 150.0;
const LOGO_CX: f32 = DESIGN_W / 2.0;
const LOGO_CY: f32 = (STRIP_Y0 + STRIP_Y1) / 2.0;
const LOGO_W: f32 = 420.0 * 1.4;
const LOGO_ALPHA: u8 = 24;
const SOFT: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x96);

const BAND_TAGS: [&str; 3] = ["low", "mid", "high"];
const BAND_RANGE: [&str; 3] = ["< 160 HZ", "160 HZ - 2.5 KHZ", "> 2.5 KHZ"];
const BAND_CODE: [&str; 3] = ["L", "M", "H"];

const FOOTER_LABEL: &str = "BANDS & GLUE";
const FOOTER_DEFAULT: &str = concat!(
    "A 3-band mastering glue. Signal path: linear-phase band split > per band: compressor that listens to the mono sum ",
    "> sine shaper on left and right > mid and side gain > sum > limiter or clipper. ",
    "The scopes draw each band's real transfer curve; the dot is the level right now. Hover controls for more details."
);
const KNOB_HINT: &str = ">DRAG = ADJUST, SHIFT + DRAG = FINE, DOUBLE-CLICK = DEFAULT";

type Tip = (&'static str, &'static str);
const TIP_THRESHOLD: Tip = ("THRESHOLD", "Level where this band's compressor starts to work. The detector hears the mono sum of the band, so pure side content never triggers it. Lower = more compression.\n");
const TIP_RATIO: Tip = ("RATIO", "How hard the band is turned down above the threshold. 1:1 = no compression. The default voicing uses very gentle ratios.\n");
const TIP_KNEE: Tip = ("KNEE", "Width of the soft transition around the threshold. A wide knee starts early and gently, a knee of 0 is a hard corner.\n");
const TIP_ATTACK: Tip = ("ATTACK", "How fast the detector follows a level rise. It tracks signal power, so the gain moves quickly at first and then settles.\n");
const TIP_RELEASE: Tip = ("RELEASE", "How fast the detector falls when the level drops. The gain comes back in a straight line in dB. Long = glue, short = lively.\n");
const TIP_BLEND: Tip = ("BLEND", "Parallel compression for this band. 100% = only the compressed signal. Lower mixes the uncompressed band back in, before the shaper, so a strong ratio can catch the peaks without flattening everything. The scope and the GR bar show the result.\n");
const TIP_DRIVE: Tip = ("DRIVE", "Saturation of this band: a sine shaper on left and right, odd harmonics only. 0 = clean. Pushed very hard, peaks fold back instead of clipping. Watch the scope.\n");
const TIP_MID: Tip = ("MID GAIN", "Level of this band's mid signal (L+R) after the shaper. Together with side gain this sets the tone curve in the response strip.\n");
const TIP_SIDE: Tip = ("SIDE GAIN", "Level of this band's side signal (L-R) after the shaper. More side than mid = wider in this band, less = narrower.\n");
const TIP_INPUT: Tip = ("INPUT", "Gain before everything. More input pushes every band harder into compression and saturation.\n");
const TIP_OUTPUT: Tip = ("OUTPUT", "Gain at the very end, after the output stage. Use it to level-match when you compare with the dry signal.\n");
const TIP_MIX: Tip = ("MIX", "Blend between the processed and the dry signal. The dry signal is delayed to line up with the processed one, so there is no phasing.\n");
const TIP_LIM_DRIVE: Tip = ("OUTPUT DRIVE", "Gain into the output stage. Peaks that pass 0 dBFS after this gain get limited (LIM) or cut flat (CLIP). More drive = louder. Does nothing when the stage is OFF.\n");
const TIP_LIM_CEILING: Tip = ("CEILING", "The level the output stage never passes. Its whole output is scaled to it. In CLIP mode peaks between samples can still read a little higher on a true-peak meter.\n");
const TIP_LIM_ATTACK: Tip = ("LIMITER ATTACK", "LIM mode only. How fast the slow stage of the limiter builds up under repeated peaks. The fast stage always catches single peaks in under half a millisecond.\n");
const TIP_LIM_RELEASE: Tip = ("LIMITER RELEASE", "LIM mode only. How fast the slow stage of the limiter lets go again. Short = louder and busier, long = steadier.\n");
const TIP_STAGE: Tip = ("OUTPUT STAGE", "What catches the peaks at the end. LIM = two-stage true-peak limiter: clean, but it pumps when pushed hard. CLIP = hard clipper: peaks are cut flat, no pumping, more grit. OFF = nothing, for when you limit or clip later in your chain.\n>CLICK = SELECT");
const TIP_CHARACTER: Tip = ("CHARACTER", "Everything in the three band panels: compression, saturation and band gains. Off = the bands pass untouched, same latency.\n>CLICK = TOGGLE");
const TIP_SCOPE: Tip = ("TRANSFER SCOPE", "Level in (left to right) against level out (bottom to top) for this band, compressor and shaper together. The dot is the band's signal level right now. The bar next to it is the gain reduction, which can recover much more slowly (see RELEASE).");
const TIP_STRIP: Tip = ("RESPONSE", "Thick lines: what the band gains do to quiet signals, 20 Hz to 20 kHz. Thin lines: the spectrum of the output, tilted 4.5 dB per octave so a balanced mix looks flat. Solid = mid (L+R), dashed = side (L-R). Where side sits above mid, the sound gets wider.\n>CLICK = SHOW THE WAVEFORM");
const TIP_WAVE: Tip = ("WAVEFORM", "The newest output, colored like a DJ waveform. Red = lows (below about 190 Hz), green = mids, blue = highs (above about 1.1 kHz); the color shows which has the most power. Height is power: 6 dB down is a quarter of the height. A drop that is mostly red has a lot of low end.\n>CLICK = SHOW THE SPECTRUM, SCROLL OR CTRL + SCROLL = ZOOM FROM 1/16 BAR TO 32 BARS");
const TIP_PAGE: Tip = ("KNOB PAGE", "Which knobs the panels show. TONE = threshold, drive, mid and side gain, and the main output controls. TIME = ratio, knee, attack, release and the limiter details. MODE = blend, shape and detect per band, and quality. Only the view changes: every knob keeps working and can be automated.
>CLICK = SELECT");
const TIP_SHAPE: Tip = ("SHAPE", "Curve of this band's saturation. SINE folds hot peaks back toward zero, the classic behavior. SOFT is the same sound at normal levels, but hot peaks are held flat instead of folding. The scope shows the difference at the top right.\n>CLICK = SELECT");
const TIP_DETECT: Tip = ("DETECT", "What this band's compressor listens to. MID hears only the mono sum (L+R), so wide content never triggers it. STEREO hears the louder of left and right, so wide bass, synths and hard-panned parts get compressed too.\n>CLICK = SELECT");
const TIP_QUALITY: Tip = ("QUALITY", "Oversampling of the saturation. 1X is the classic sound: harmonics above half the sample rate fold back as extra tones. 2X runs the shapers at twice the sample rate for cleaner highs. It costs more CPU.\n>CLICK = SELECT");
const TIP_PRESET: Tip = ("PRESET", "Click the name to pick a preset, or step through the list with the arrows. The disk saves the current settings under a name you type. A star after the name means a knob moved since the preset was loaded.");
const TIP_METER: Tip = ("OUTPUT STAGE REDUCTION", "LIM: how much the limiter turns the signal down right now. CLIP: how far the peaks are over the ceiling, which is how much gets cut off. Scale 0 to -12 dB.");

pub struct SanctisEditor {
    params: Arc<SanctisParams>,
    meters: Arc<Meters>,
    gui_ctx: Option<GuiContext>,
    logo: Option<egui::TextureHandle>,
    /// Smoothed meter values: band gain reduction x3, limiter.
    shown_gr: [f32; 4],
    shown_level: [f32; 3],
    analyzer: Analyzer,
    /// Which knobs the panels show: 0 = TONE, 1 = TIME, 2 = MODE. View only, not a parameter.
    page: usize,
    presets: PresetBar,
    /// The strip shows the colored waveform instead of the response and spectrum. View only.
    wave_view: bool,
    /// Zoom: index into `wave::SPANS`.
    wave_span: usize,
    /// Mouse wheel travel that has not made a zoom step yet.
    wave_wheel: f32,
    wave_bins: Vec<wave::Bin>,
    /// Debug builds only: draw a made-up waveform (layout screenshots).
    wave_demo: bool,
}

impl SanctisEditor {
    pub fn new(params: Arc<SanctisParams>, meters: Arc<Meters>) -> Self {
        // debug builds only: SANCTIS_UI_PAGE=1 / 2 opens on the TIME / MODE page (for layout screenshots)
        let page = if cfg!(debug_assertions) { std::env::var("SANCTIS_UI_PAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize).min(2) } else { 0 };
        // debug builds only: SANCTIS_UI_WAVE=1 opens on the waveform with made-up content
        let wave_demo = cfg!(debug_assertions) && std::env::var_os("SANCTIS_UI_WAVE").is_some();
        let presets = PresetBar::new("C-SANCTIS", params.clone(), &SanctisParams::default(), params.preset.clone(), crate::FACTORY, &[]);
        Self { params, meters, gui_ctx: None, logo: None, shown_gr: [0.0; 4], shown_level: [SCOPE_MIN; 3], analyzer: Analyzer::default(), page, presets, wave_view: wave_demo, wave_span: wave::DEFAULT_SPAN, wave_wheel: 0.0, wave_bins: Vec::with_capacity(wave::SCREEN), wave_demo }
    }
}

/// Band weights (low, mid, high) at a frequency: the targets of the engine's split filters.
fn band_weights(f: f32) -> [f32; 3] {
    let low = 1.0 / (1.0 + (f / SPLIT_LO_HZ).powf(6.667));
    let lp = 1.0 / (1.0 + (f / SPLIT_HI_HZ).powf(4.307));
    [low, lp - low, 1.0 - lp]
}

/// Static transfer of one band: detector level in -> band level out (dBFS, before the band gains).
fn band_transfer_db(level_db: f32, k: &BandKnobs) -> f32 {
    let slope = 1.0 / k.ratio.value().max(1.0) - 1.0;
    let gr = gain_reduction_db(level_db, k.threshold.value(), slope, k.knee.value().max(1e-3));
    let a = 10f32.powf(level_db / 20.0) * (1.0 + k.blend.value() * (10f32.powf(gr / 20.0) - 1.0));
    let d = k.drive.value();
    let u = if k.shape.value() == ShapeMode::Soft { (d * a).min(std::f32::consts::FRAC_PI_2) } else { d * a };
    let y = if d > 0.0 { (u.sin() / d).abs() } else { a };
    20.0 * y.max(1e-6).log10()
}

/// Made-up bins for layout screenshots: a kick, a bass note and hats per beat.
fn wave_demo(span: usize, bins: &mut Vec<wave::Bin>) {
    let n = span.min(wave::SCREEN);
    let per_beat = (n * wave::FINE_PER_BAR / span / 4).max(1);
    bins.clear();
    for i in 0..n {
        let beat = (i % per_beat) as f32 / per_beat as f32;
        let (rgb, height) = if beat < 0.12 {
            ([255, 60, 40], 1.0 - 2.0 * beat)
        } else if beat >= 0.5 {
            ([120, 200, 255], 0.35 + 0.1 * ((i * 7) % 5) as f32 / 5.0)
        } else {
            ([255, 150, 90], 0.7 - 0.2 * beat)
        };
        bins.push(wave::Bin { rgb, height });
    }
}

fn db(g: f32) -> f32 {
    20.0 * g.max(1e-6).log10()
}

/// Captioned segmented switch for an enum parameter (MODE page). Caption sits 24 above the pill centre.
#[allow(clippy::too_many_arguments)]
fn mode_switch<E: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    c: &Canvas,
    cx: f32,
    cy: f32,
    id: &str,
    label: &str,
    names: &[&str],
    seg_w: f32,
    values: &[E],
    param: &EnumParam<E>,
    setter: &ParamSetter,
) -> egui::Response {
    let cur = values.iter().position(|v| *v == param.value()).unwrap_or(0);
    c.text(ui, cx, cy - 24.0, Align2::CENTER_BOTTOM, label, 10.0, MEDIUM, WHITE);
    let (resp, picked) = ui::segmented(ui, c, cx, cy, id, names, seg_w, cur, false);
    if let Some(i) = picked.filter(|i| *i != cur) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, values[i]);
        setter.end_set_parameter(param);
    }
    resp
}

impl NiceEguiApp for SanctisEditor {
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
        let bands = p.bands();

        // meters: instant attack, smooth fall
        let raw_gr = [
            self.meters.gr[0].load(Ordering::Relaxed),
            self.meters.gr[1].load(Ordering::Relaxed),
            self.meters.gr[2].load(Ordering::Relaxed),
            self.meters.limiter.load(Ordering::Relaxed),
        ];
        for (s, v) in self.shown_gr.iter_mut().zip(raw_gr) {
            *s = if v < *s { v } else { *s + 0.15 * (v - *s) };
        }
        for (s, m) in self.shown_level.iter_mut().zip(&self.meters.level) {
            let v = m.load(Ordering::Relaxed).clamp(SCOPE_MIN, SCOPE_MAX);
            *s = if v > *s { v } else { *s + 0.15 * (v - *s) };
        }
        let now = ui.input(|i| i.time);
        let spectrum_alive = self.analyzer.update(&self.meters.spectrum, now);
        if spectrum_alive || self.shown_gr.iter().any(|v| *v < -0.01) || self.shown_level.iter().any(|v| *v > SCOPE_MIN + 0.1) {
            ui.ctx().request_repaint();
        }

        let avail = ui.max_rect();
        ui.painter().rect_filled(avail, 0.0, PAPER);
        let c = Canvas::fit(avail, DESIGN);
        let right = DESIGN_W - MARGIN;

        // which control the footer explains: (title, body), is a knob
        let mut hovered: Option<(Tip, bool)> = None;
        let mut track = |resp: egui::Response, tip: Tip, is_knob: bool| {
            if resp.hovered() || resp.dragged() {
                hovered = Some((tip, is_knob));
            }
        };

        // ---- header ----
        for (i, line) in [HEAD_L, "ANSELIC // BY AZUR1S"].iter().enumerate() {
            c.text(ui, MARGIN - 2.0, 18.0 + 13.0 * i as f32, Align2::LEFT_TOP, line, 10.0, MEDIUM, INK);
        }
        for (i, line) in [HEAD_R, concat!("VER ", env!("CARGO_PKG_VERSION"))].iter().enumerate() {
            c.text(ui, right, 18.0 + 13.0 * i as f32, Align2::RIGHT_TOP, line, 10.0, MEDIUM, INK);
        }
        track(self.presets.show(ui, &c, BAR_X0, BAR_X1, BAR_CY, gui_ctx), TIP_PRESET, false);

        // ---- response strip ----
        let strip = c.r(MARGIN, STRIP_Y0, right, STRIP_Y1);
        ui.painter().rect_filled(strip, c.l(8.0), INK);
        if let Some(logo) = &self.logo {
            let (w, h) = (LOGO_W, LOGO_W / ui::LOGO_ASPECT);
            let (cx, cy) = (LOGO_CX, LOGO_CY);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().with_clip_rect(strip).image(
                logo.id(),
                c.r(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0),
                uv,
                Color32::from_white_alpha(LOGO_ALPHA),
            );
        }
        if self.wave_view {
            let span = wave::SPANS[self.wave_span];
            if self.wave_demo {
                wave_demo(span, &mut self.wave_bins);
            } else {
                self.meters.wave.bins(span, &mut self.wave_bins);
            }
            // mirrored around the middle line, newest on the right
            let (x0, x1, y0, y1) = (MARGIN + 16.0, right - 16.0, STRIP_Y0 + 24.0, STRIP_Y1 - 16.0);
            let (mid, half) = ((y0 + y1) / 2.0, (y1 - y0) / 2.0);
            let dx = (x1 - x0) / self.wave_bins.len() as f32;
            c.line(ui, &[(x0, mid), (x1, mid)], 1.0, FAINT);
            for (i, bin) in self.wave_bins.iter().enumerate() {
                if bin.height > 0.0 {
                    let (x, h) = (x0 + dx * i as f32, (half * bin.height).max(0.5));
                    let [r, g, b] = bin.rgb;
                    ui.painter().rect_filled(c.r(x, mid - h, x + dx, mid + h), 0.0, Color32::from_rgb(r, g, b));
                }
            }
            let title = format!("WAVEFORM // OUTPUT, LAST {}", wave::SPAN_NAMES[self.wave_span]);
            c.text(ui, x0, STRIP_Y0 + 8.0, Align2::LEFT_TOP, &title, 8.0, REGULAR, DIM);
            for (i, (name, col)) in [("LOW", Color32::from_rgb(255, 0, 0)), ("MID", Color32::from_rgb(0, 255, 0)), ("HIGH", Color32::from_rgb(0, 96, 255))].into_iter().enumerate() {
                c.text(ui, x1 - 34.0 * (2 - i) as f32, STRIP_Y0 + 8.0, Align2::RIGHT_TOP, name, 8.0, REGULAR, col);
            }
        } else {
            let (x0, x1, y0, y1) = (MARGIN + 44.0, right - 16.0, STRIP_Y0 + 26.0, STRIP_Y1 - 18.0);
            let fx = |f: f32| x0 + (x1 - x0) * (f / 20.0).log10() / 3.0;
            let gy = |g_db: f32| (y0 + y1) / 2.0 - (y1 - y0) / 2.0 * (g_db / STRIP_DB).clamp(-1.0, 1.0);
            // grid: 0 dB line, +-6 dB, the two splits, decade ticks
            c.line(ui, &[(x0, gy(0.0)), (x1, gy(0.0))], 1.0, FAINT);
            for g in [-6.0f32, -3.0, 3.0, 6.0] {
                c.line(ui, &[(x0, gy(g)), (x1, gy(g))], 0.6, FAINT);
            }
            for (g, name) in [(-6.0f32, "-6"), (0.0, "0"), (6.0, "+6")] {
                c.text(ui, x0 - 6.0, gy(g), Align2::RIGHT_CENTER, name, 7.5, REGULAR, DIM);
            }
            for f in [SPLIT_LO_HZ, SPLIT_HI_HZ] {
                c.line(ui, &[(fx(f), y0), (fx(f), y1)], 1.0, DIM);
            }
            for (f, name) in [(20.0, "20"), (100.0, "100"), (1000.0, "1K"), (10000.0, "10K"), (20000.0, "20K")] {
                c.text(ui, fx(f), y1 + 3.0, Align2::CENTER_TOP, name, 7.5, REGULAR, DIM);
            }
            // curves: 0 dB when Character is off
            let on = p.character.value();
            let gains = |side: bool| -> [f32; 3] {
                std::array::from_fn(|k| if !on { 1.0 } else { 10f32.powf((if side { bands[k].side.value() } else { bands[k].mid.value() }) / 20.0) })
            };
            let curve = |g: [f32; 3]| -> Vec<(f32, f32)> {
                (0..=160)
                    .map(|i| {
                        let f = 20.0 * 1000f32.powf(i as f32 / 160.0);
                        let w = band_weights(f);
                        (fx(f), gy(db(g[0] * w[0] + g[1] * w[1] + g[2] * w[2])))
                    })
                    .collect()
            };
            // output spectrum behind the response curves: mid solid, side dotted
            let sy = |d: f32| y1 - (y1 - y0) * ((d - SPEC_BOTTOM_DB) / (SPEC_TOP_DB - SPEC_BOTTOM_DB)).clamp(0.0, 1.0);
            let spec = |ch: usize| -> Vec<(f32, f32)> {
                self.analyzer.shown[ch].iter().enumerate().map(|(i, d)| (fx(spectrum::point_freq(i)), sy(*d))).collect()
            };
            if spectrum_alive {
                // colored like the waveform: the color a tone at that frequency would get there.
                // Lifted a little toward white so blue stays readable on black; side is dimmer.
                let tint = |i: usize, side: bool| {
                    let k = if side { 0.5 } else { 1.0 };
                    let [r, g, b] = wave::tone_color(spectrum::point_freq(i)).map(|v| (k * (0.75 * v as f32 + 0.25 * 255.0)) as u8);
                    Color32::from_rgb(r, g, b)
                };
                // only the parts that rise above the bottom edge of the plot
                let floor_y = sy(SPEC_BOTTOM_DB) - 0.5;
                for (ch, dashed) in [(1, true), (0, false)] {
                    let pts = spec(ch);
                    for i in 0..pts.len() - 1 {
                        if pts[i].1 >= floor_y || pts[i + 1].1 >= floor_y || (dashed && i % 4 >= 2) {
                            continue;
                        }
                        c.line(ui, &pts[i..i + 2], if dashed { 1.0 } else { 1.2 }, tint(i, dashed));
                    }
                }
            }
            // side: dashed (every other piece), mid: solid
            for piece in curve(gains(true)).chunks(3).step_by(2) {
                c.line(ui, piece, 1.6, SOFT);
            }
            c.line(ui, &curve(gains(false)), 1.8, WHITE);
            c.text(ui, x0 + 4.0, STRIP_Y0 + 8.0, Align2::LEFT_TOP, "RESPONSE // BAND GAINS + OUTPUT SPECTRUM", 8.0, REGULAR, DIM);
            c.text(ui, x1, STRIP_Y0 + 8.0, Align2::RIGHT_TOP, "MID ---   SIDE - -", 8.0, REGULAR, DIM);
        }
        let strip_area = ui.interact(strip, ui.id().with("strip"), egui::Sense::click());
        if strip_area.clicked() {
            self.wave_view = !self.wave_view;
        }
        if self.wave_view && strip_area.hovered() {
            // wheel up = zoom in. With Ctrl held egui reports the wheel as a zoom factor instead.
            let (wheel, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            self.wave_wheel += wheel / 50.0 + 4.0 * zoom.ln();
            while self.wave_wheel.abs() >= 1.0 {
                let up = self.wave_wheel > 0.0;
                self.wave_wheel -= if up { 1.0 } else { -1.0 };
                self.wave_span = if up { self.wave_span.saturating_sub(1) } else { (self.wave_span + 1).min(wave::SPANS.len() - 1) };
            }
        } else {
            self.wave_wheel = 0.0;
        }
        track(strip_area, if self.wave_view { TIP_WAVE } else { TIP_STRIP }, false);

        // page switch: view only, so it sits outside the panels, on the tag row
        // centred in the gap between the strip and the panels
        let sw_y = (STRIP_Y1 + PANEL_Y0) / 2.0;
        c.text(ui, right - 150.0, sw_y, Align2::RIGHT_CENTER, "KNOB PAGE", 9.0, MEDIUM, INK);
        let (sw, picked) = ui::segmented(ui, &c, right - 69.0, sw_y, "page", &["TONE", "TIME", "MODE"], 46.0, self.page, true);
        if let Some(i) = picked {
            self.page = i;
        }
        track(sw, TIP_PAGE, false);
        let page = self.page;

        let small = |bipolar: bool| KnobStyle { radius: KNOB_R, bipolar };
        let knob_x = |px: f32, i: usize| px + KNOB_X0 + KNOB_DX * i as f32;

        // ---- band panels ----
        for (k, band) in bands.iter().enumerate() {
            let px = MARGIN + (PANEL_W + PANEL_GAP) * k as f32;
            ui::tag(ui, &c, px, TAG_Y, BAND_TAGS[k]);
            ui.painter().rect_filled(c.r(px, PANEL_Y0, px + PANEL_W, PANEL_Y1), c.l(8.0), INK);
            c.line(ui, &[(px + 16.0, PANEL_Y0 + 18.0), (px + PANEL_W - 16.0, PANEL_Y0 + 18.0)], 2.0, WHITE);
            c.text(ui, px + 16.0, PANEL_Y0 + 24.0, Align2::LEFT_TOP, "TRANSFER // IN > OUT", 8.0, REGULAR, DIM);
            c.text(ui, px + PANEL_W - 16.0, PANEL_Y0 + 24.0, Align2::RIGHT_TOP, BAND_RANGE[k], 8.0, REGULAR, DIM);

            // scope
            let (sx0, sx1) = (px + 16.0, px + 240.0);
            let sx = |l: f32| sx0 + (sx1 - sx0) * (l - SCOPE_MIN) / (SCOPE_MAX - SCOPE_MIN);
            let sy = |l: f32| SCOPE_Y1 - (SCOPE_Y1 - SCOPE_Y0) * (l.clamp(SCOPE_MIN, SCOPE_MAX) - SCOPE_MIN) / (SCOPE_MAX - SCOPE_MIN);
            ui.painter().rect_stroke(c.r(sx0, SCOPE_Y0, sx1, SCOPE_Y1), 0.0, egui::Stroke::new(c.l(0.8), FAINT), egui::StrokeKind::Inside);
            for g in [-36.0f32, -24.0, -12.0, 0.0] {
                c.line(ui, &[(sx(g), SCOPE_Y0), (sx(g), SCOPE_Y1)], 0.6, FAINT);
                c.line(ui, &[(sx0, sy(g)), (sx1, sy(g))], 0.6, FAINT);
            }
            c.text(ui, sx(0.0), SCOPE_Y1 + 3.0, Align2::CENTER_TOP, "0", 7.5, REGULAR, DIM);
            c.text(ui, sx(-24.0), SCOPE_Y1 + 3.0, Align2::CENTER_TOP, "-24", 7.5, REGULAR, DIM);
            // unity line, threshold tick, curve, live dot
            c.line(ui, &[(sx(SCOPE_MIN), sy(SCOPE_MIN)), (sx(SCOPE_MAX), sy(SCOPE_MAX))], 0.8, DIM);
            let thr = band.threshold.value();
            if thr > SCOPE_MIN {
                c.line(ui, &[(sx(thr), SCOPE_Y1 - 7.0), (sx(thr), SCOPE_Y1)], 1.6, WHITE);
            }
            let on = p.character.value();
            let transfer = |l: f32| if on { band_transfer_db(l, band) } else { l };
            let pts: Vec<(f32, f32)> = (0..=108).map(|i| SCOPE_MIN + 0.5 * i as f32).map(|l| (sx(l), sy(transfer(l)))).collect();
            c.line(ui, &pts, 1.8, WHITE);
            let lvl = self.shown_level[k];
            if lvl > SCOPE_MIN + 0.5 {
                c.line(ui, &[(sx(lvl), SCOPE_Y0), (sx(lvl), SCOPE_Y1)], 0.8, DIM);
                ui.painter().circle_filled(c.p(sx(lvl), sy(transfer(lvl))), c.l(3.2), WHITE);
            }
            // gain reduction ladder next to the scope
            ui::ladder(ui, &c, px + 258.0, SCOPE_Y0, SCOPE_Y1, 14.0, self.shown_gr[k], METER_RANGE_DB);
            c.text(ui, px + 265.0, SCOPE_Y1 + 3.0, Align2::CENTER_TOP, "GR", 7.5, REGULAR, if self.shown_gr[k] < -0.1 { WHITE } else { DIM });
            let scope_area = ui.interact(c.r(sx0, SCOPE_Y0, px + PANEL_W - 16.0, SCOPE_Y1), ui.id().with(("scope", k)), egui::Sense::hover());
            track(scope_area, TIP_SCOPE, false);

            c.line(ui, &[(px + 16.0, SCOPE_Y1 + 22.0), (px + PANEL_W - 16.0, SCOPE_Y1 + 22.0)], 1.0, FAINT);
            let code = |i: usize| format!("#{}{}", BAND_CODE[k], i);
            if page == 0 {
                track(ui::knob(ui, &c, knob_x(px, 0), ROW, &code(1), "THRESH", &band.threshold, &setter, small(false)), TIP_THRESHOLD, true);
                track(ui::knob(ui, &c, knob_x(px, 1), ROW, &code(2), "DRIVE", &band.drive, &setter, small(false)), TIP_DRIVE, true);
                track(ui::knob(ui, &c, knob_x(px, 2), ROW, &code(3), "MID", &band.mid, &setter, small(true)), TIP_MID, true);
                track(ui::knob(ui, &c, knob_x(px, 3), ROW, &code(4), "SIDE", &band.side, &setter, small(true)), TIP_SIDE, true);
            } else if page == 1 {
                track(ui::knob(ui, &c, knob_x(px, 0), ROW, &code(5), "RATIO", &band.ratio, &setter, small(false)), TIP_RATIO, true);
                track(ui::knob(ui, &c, knob_x(px, 1), ROW, &code(6), "KNEE", &band.knee, &setter, small(false)), TIP_KNEE, true);
                track(ui::knob(ui, &c, knob_x(px, 2), ROW, &code(7), "ATTACK", &band.attack, &setter, small(false)), TIP_ATTACK, true);
                track(ui::knob(ui, &c, knob_x(px, 3), ROW, &code(8), "RELEASE", &band.release, &setter, small(false)), TIP_RELEASE, true);
            } else {
                // knob in the left half, two switches stacked in the right half
                let (cx1, cx2) = (px + PANEL_W * 0.25, px + PANEL_W * 0.75);
                track(ui::knob(ui, &c, cx1, ROW, &code(11), "BLEND", &band.blend, &setter, small(false)), TIP_BLEND, true);
                track(mode_switch(ui, &c, cx2, MODE_Y_TOP, &code(10), "DETECT", &["MID", "STEREO"], 54.0, &[DetectMode::Mid, DetectMode::Stereo], &band.detect, &setter), TIP_DETECT, false);
                track(mode_switch(ui, &c, cx2, MODE_Y_BOTTOM, &code(9), "SHAPE", &["SINE", "SOFT"], 54.0, &[ShapeMode::Sine, ShapeMode::Soft], &band.shape, &setter), TIP_SHAPE, false);
            }
        }

        // ---- output panel ----
        let px = MARGIN + (PANEL_W + PANEL_GAP) * 3.0;
        ui::tag(ui, &c, px, TAG_Y, "output");
        ui.painter().rect_filled(c.r(px, PANEL_Y0, px + PANEL_W, PANEL_Y1), c.l(8.0), INK);
        c.line(ui, &[(px + 16.0, PANEL_Y0 + 18.0), (px + PANEL_W - 16.0, PANEL_Y0 + 18.0)], 2.0, WHITE);
        let stage = p.stage.value();
        let stage_line = match stage {
            StageMode::Off => "OUTPUT // OPEN",
            StageMode::Limit => "OUTPUT // TRUE-PEAK LIMIT",
            StageMode::Clip => "OUTPUT // HARD CLIP",
        };
        c.text(ui, px + 16.0, PANEL_Y0 + 24.0, Align2::LEFT_TOP, stage_line, 8.0, REGULAR, DIM);
        c.text(ui, px + PANEL_W - 16.0, PANEL_Y0 + 24.0, Align2::RIGHT_TOP, "MODEL // ANSLC", 8.0, REGULAR, DIM);

        // two switches left, output stage meter right. Each switch is 34 tall (caption top to pill
        // bottom, pill centre 24 below the caption top, 14 of air between caption and pill).
        // The gap between two switches is twice that air, so a caption reads with its own pill;
        // what is left goes above and below.
        const SW_H: f32 = 34.0;
        const SW_GAP: f32 = 28.0;
        let sw_pad = (SCOPE_Y1 - SCOPE_Y0 - 2.0 * SW_H - SW_GAP) / 2.0;
        let sw_cy = |i: usize| SCOPE_Y0 + sw_pad + 24.0 + (SW_H + SW_GAP) * i as f32;
        let modes = [StageMode::Off, StageMode::Limit, StageMode::Clip];
        track(mode_switch(ui, &c, px + 96.0, sw_cy(0), "#O1", "OUTPUT STAGE", &["OFF", "LIM", "CLIP"], 44.0, &modes, &p.stage, &setter), TIP_STAGE, false);
        let on = p.character.value();
        let sw = ui::switch(ui, &c, px + 96.0, sw_cy(1), "#O2", "CHARACTER", on);
        if sw.clicked() {
            setter.begin_set_parameter(&p.character);
            setter.set_parameter(&p.character, !on);
            setter.end_set_parameter(&p.character);
        }
        track(sw, TIP_CHARACTER, false);


        let (mx, my0, my1) = (px + 226.0, SCOPE_Y0, SCOPE_Y1);
        for i in 0..=4 {
            let g = -3.0 * i as f32;
            let y = my0 + (my1 - my0) * (-g / METER_RANGE_DB);
            c.text(ui, mx - 10.0, y, Align2::RIGHT_CENTER, &format!("{g:.0}"), 7.5, REGULAR, DIM);
            c.line(ui, &[(mx - 7.0, y), (mx - 3.0, y)], 1.0, DIM);
        }
        let lim = self.shown_gr[3];
        ui::ladder(ui, &c, mx, my0, my1, 26.0, lim, METER_RANGE_DB);
        let meter_name = if stage == StageMode::Clip { "CLIP" } else { "LIM" };
        c.text(ui, mx + 13.0, my1 + 3.0, Align2::CENTER_TOP, meter_name, 7.5, REGULAR, if lim < -0.1 { WHITE } else { DIM });
        let meter_area = ui.interact(c.r(mx - 30.0, my0, mx + 30.0, my1 + 14.0), ui.id().with("lim-meter"), egui::Sense::hover());
        track(meter_area, TIP_METER, false);

        c.line(ui, &[(px + 16.0, SCOPE_Y1 + 22.0), (px + PANEL_W - 16.0, SCOPE_Y1 + 22.0)], 1.0, FAINT);
        if page == 0 {
            track(ui::knob(ui, &c, knob_x(px, 0), ROW, "#O3", "INPUT", &p.input, &setter, small(true)), TIP_INPUT, true);
            track(ui::knob(ui, &c, knob_x(px, 1), ROW, "#O4", "DRIVE", &p.lim_drive, &setter, small(false)), TIP_LIM_DRIVE, true);
            track(ui::knob(ui, &c, knob_x(px, 2), ROW, "#O5", "OUTPUT", &p.output, &setter, small(true)), TIP_OUTPUT, true);
            track(ui::knob(ui, &c, knob_x(px, 3), ROW, "#O6", "MIX", &p.mix, &setter, small(false)), TIP_MIX, true);
        } else if page == 1 {
            track(ui::knob(ui, &c, knob_x(px, 0), ROW, "#O7", "CEILING", &p.lim_ceiling, &setter, small(false)), TIP_LIM_CEILING, true);
            track(ui::knob(ui, &c, knob_x(px, 1), ROW, "#O8", "LIM ATT", &p.lim_attack, &setter, small(false)), TIP_LIM_ATTACK, true);
            track(ui::knob(ui, &c, knob_x(px, 2), ROW, "#O9", "LIM REL", &p.lim_release, &setter, small(false)), TIP_LIM_RELEASE, true);
        } else {
            track(mode_switch(ui, &c, px + PANEL_W * 0.5, MODE_Y, "#O10", "QUALITY", &["1X", "2X"], 46.0, &[Quality::X1, Quality::X2], &p.quality, &setter), TIP_QUALITY, false);
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
            (TIP_THRESHOLD, true),
            (TIP_RATIO, true),
            (TIP_KNEE, true),
            (TIP_ATTACK, true),
            (TIP_RELEASE, true),
            (TIP_DRIVE, true),
            (TIP_BLEND, true),
            (TIP_MID, true),
            (TIP_SIDE, true),
            (TIP_INPUT, true),
            (TIP_OUTPUT, true),
            (TIP_MIX, true),
            (TIP_LIM_DRIVE, true),
            (TIP_LIM_CEILING, true),
            (TIP_LIM_ATTACK, true),
            (TIP_LIM_RELEASE, true),
            (TIP_STAGE, false),
            (TIP_CHARACTER, false),
            (TIP_SCOPE, false),
            (TIP_STRIP, false),
            (TIP_WAVE, false),
            (TIP_METER, false),
            (TIP_PRESET, false),
            (TIP_PAGE, false),
            (TIP_SHAPE, false),
            (TIP_DETECT, false),
            (TIP_QUALITY, false),
        ] {
            all.push((tip.0, if knob { format!("{}{KNOB_HINT}", tip.1) } else { tip.1.to_owned() }));
        }
        for (label, text) in all {
            let n = ui::fine_print_lines(FOOTER_W, &text).len();
            assert!(n <= ui::FINE_LINES, "{label}: needs {n} lines, footer has {}", ui::FINE_LINES);
        }
    }

    #[test]
    fn band_weights_sum_to_one() {
        for f in [20.0, 100.0, 161.0, 500.0, 2509.0, 8000.0, 20000.0] {
            let w = band_weights(f);
            assert!((w[0] + w[1] + w[2] - 1.0).abs() < 1e-6);
        }
        assert!((band_weights(SPLIT_LO_HZ)[0] - 0.5).abs() < 1e-3);
    }
}
