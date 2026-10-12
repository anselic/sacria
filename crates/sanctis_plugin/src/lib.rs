//! Plugin wrapper around `sanctis_engine::Engine`.
//!
//! Every knob maps straight to one engine parameter. At their defaults the
//! plugin is the engine's reference voicing (`Params::reference()`).

use atomic_float::AtomicF32;
use nice_plug::prelude::*;
use nice_plug_egui::{create_egui_editor, EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};
use sanctis_engine::{BandParams, Detect, Engine, LimiterParams, OutputMode, Params as CoreParams, Shape, NUM_BANDS};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

mod editor;
mod spectrum;
mod wave;

/// Values for the GUI: band gain reduction (dB, <= 0), band detector level
/// (dBFS), output stage gain reduction (dB, <= 0).
pub struct Meters {
    pub gr: [AtomicF32; NUM_BANDS],
    pub level: [AtomicF32; NUM_BANDS],
    pub limiter: AtomicF32,
    /// Output samples for the spectrum display.
    pub spectrum: spectrum::SpectrumTap,
    /// Columns of the colored waveform display.
    pub wave: wave::WaveTap,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            gr: std::array::from_fn(|_| AtomicF32::new(0.0)),
            level: std::array::from_fn(|_| AtomicF32::new(-200.0)),
            limiter: AtomicF32::new(0.0),
            spectrum: spectrum::SpectrumTap::default(),
            wave: wave::WaveTap::default(),
        }
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeMode {
    #[id = "sine"]
    #[name = "Sine"]
    Sine,
    #[id = "soft"]
    #[name = "Soft"]
    Soft,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectMode {
    #[id = "mid"]
    #[name = "Mid"]
    Mid,
    #[id = "stereo"]
    #[name = "Stereo"]
    Stereo,
}

/// The output stage: off, the true-peak limiter, or a hard clipper.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageMode {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "limit"]
    #[name = "Limit"]
    Limit,
    #[id = "clip"]
    #[name = "Clip"]
    Clip,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    #[id = "1x"]
    #[name = "1x"]
    X1,
    #[id = "2x"]
    #[name = "2x"]
    X2,
}

/// Built into the plugin: only the starting point. Every other preset is a file in the
/// user's preset folder. The ones we ship are in `presets/C-SANCTIS/` in the repo root and go into the release zip.
pub const FACTORY: &[sacria_ui::presets::Factory] = &[sacria_ui::presets::Factory { name: sacria_ui::presets::DEFAULT_NAME, category: "", params: "{}" }];

/// The knobs of one band.
#[derive(Params)]
pub struct BandKnobs {
    #[id = "thr"]
    pub threshold: FloatParam,
    #[id = "ratio"]
    pub ratio: FloatParam,
    #[id = "knee"]
    pub knee: FloatParam,
    #[id = "att"]
    pub attack: FloatParam,
    #[id = "rel"]
    pub release: FloatParam,
    /// Parallel compression: 100% = only the compressed band.
    #[id = "blend"]
    pub blend: FloatParam,
    #[id = "drive"]
    pub drive: FloatParam,
    #[id = "mid"]
    pub mid: FloatParam,
    #[id = "side"]
    pub side: FloatParam,
    /// Shaper curve: sine (folds when hot) or soft (holds the top).
    #[id = "shape"]
    pub shape: EnumParam<ShapeMode>,
    /// What the compressor listens to: the mono sum, or the louder of left and right.
    #[id = "detect"]
    pub detect: EnumParam<DetectMode>,
}

fn ms_param(name: String, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Skewed { min, max, factor: FloatRange::skew_factor(-2.0) })
        .with_unit(" ms")
        .with_value_to_string(Arc::new(|v| if v < 10.0 { format!("{v:.2}") } else if v < 100.0 { format!("{v:.1}") } else { format!("{v:.0}") }))
}

fn db_param(name: String, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max })
        .with_unit(" dB")
        .with_step_size(0.1)
        .with_value_to_string(formatters::v2s_f32_rounded(1))
}

impl BandKnobs {
    fn new(band: &str, d: &BandParams) -> Self {
        let n = |what: &str| format!("{band} {what}");
        Self {
            threshold: db_param(n("Threshold"), d.threshold_db, -60.0, 0.0),
            ratio: FloatParam::new(n("Ratio"), d.ratio, FloatRange::Skewed { min: 1.0, max: 10.0, factor: FloatRange::skew_factor(-2.0) })
                .with_unit(":1")
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            knee: db_param(n("Knee"), d.knee_db, 0.0, 40.0),
            attack: ms_param(n("Attack"), d.attack_ms, 0.1, 500.0),
            release: ms_param(n("Release"), d.release_ms, 5.0, 5000.0),
            blend: FloatParam::new(n("Blend"), d.blend, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            drive: FloatParam::new(n("Drive"), d.drive, FloatRange::Linear { min: 0.0, max: 5.0 })
                .with_step_size(0.01)
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            mid: db_param(n("Mid Gain"), d.mid_gain_db, -12.0, 12.0),
            side: db_param(n("Side Gain"), d.side_gain_db, -12.0, 12.0),
            shape: EnumParam::new(n("Shape"), ShapeMode::Sine),
            detect: EnumParam::new(n("Detect"), DetectMode::Mid),
        }
    }

    fn band_params(&self) -> BandParams {
        BandParams {
            threshold_db: self.threshold.value(),
            ratio: self.ratio.value(),
            knee_db: self.knee.value(),
            attack_ms: self.attack.value(),
            release_ms: self.release.value(),
            blend: self.blend.value(),
            drive: self.drive.value(),
            mid_gain_db: self.mid.value(),
            side_gain_db: self.side.value(),
            shape: if self.shape.value() == ShapeMode::Soft { Shape::Soft } else { Shape::Sine },
            detect: if self.detect.value() == DetectMode::Stereo { Detect::Stereo } else { Detect::Mid },
        }
    }
}

#[derive(Params)]
pub struct SanctisParams {
    /// Name of the loaded preset, for the preset bar.
    #[persist = "preset"]
    pub preset: Arc<RwLock<String>>,

    #[id = "input"]
    pub input: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    /// Compression, saturation and band gains. Off = the bands pass untouched.
    #[id = "character"]
    pub character: BoolParam,

    #[nested(group = "Low", id_prefix = "low")]
    pub low: BandKnobs,
    #[nested(group = "Mid", id_prefix = "mid")]
    pub mid: BandKnobs,
    #[nested(group = "High", id_prefix = "high")]
    pub high: BandKnobs,

    /// What catches the peaks at the end.
    #[id = "stage"]
    pub stage: EnumParam<StageMode>,
    #[id = "lim_drive"]
    pub lim_drive: FloatParam,
    #[id = "lim_ceil"]
    pub lim_ceiling: FloatParam,
    #[id = "lim_att"]
    pub lim_attack: FloatParam,
    #[id = "lim_rel"]
    pub lim_release: FloatParam,

    /// Shaper oversampling.
    #[id = "quality"]
    pub quality: EnumParam<Quality>,
}

impl SanctisParams {
    pub fn bands(&self) -> [&BandKnobs; NUM_BANDS] {
        [&self.low, &self.mid, &self.high]
    }
}

impl Default for SanctisParams {
    fn default() -> Self {
        let d = CoreParams::reference();
        let gain = |name: &str| {
            FloatParam::new(name, 1.0, FloatRange::gain_range(-12.0, 12.0))
                .with_smoother(SmoothingStyle::Logarithmic(30.0))
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
                .with_string_to_value(formatters::s2v_f32_gain_to_db())
        };
        Self {
            preset: Arc::new(RwLock::new(sacria_ui::presets::DEFAULT_NAME.to_owned())),
            input: gain("Input"),
            output: gain("Output"),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(30.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            character: BoolParam::new("Character", true),
            low: BandKnobs::new("Low", &d.bands[0]),
            mid: BandKnobs::new("Mid", &d.bands[1]),
            high: BandKnobs::new("High", &d.bands[2]),
            stage: EnumParam::new("Output Stage", StageMode::Limit),
            lim_drive: db_param("Output Drive".into(), d.limiter.input_gain_db, 0.0, 12.0),
            lim_ceiling: db_param("Limiter Ceiling".into(), d.limiter.ceiling_db, -6.0, 0.0),
            lim_attack: ms_param("Limiter Attack".into(), d.limiter.slow_attack_ms, 1.0, 1000.0),
            lim_release: ms_param("Limiter Release".into(), d.limiter.slow_release_ms, 10.0, 2000.0),
            quality: EnumParam::new("Quality", Quality::X1),
        }
    }
}

/// Delays the dry signal by the engine latency so Mix does not comb-filter.
#[derive(Default)]
struct DryDelay {
    buf: Vec<[f32; 2]>,
    pos: usize,
}

impl DryDelay {
    fn resize(&mut self, len: usize) {
        self.buf = vec![[0.0; 2]; len];
        self.pos = 0;
    }

    fn clear(&mut self) {
        self.buf.fill([0.0; 2]);
        self.pos = 0;
    }

    #[inline]
    fn tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        if self.buf.is_empty() {
            return x;
        }
        let y = std::mem::replace(&mut self.buf[self.pos], x);
        self.pos = if self.pos + 1 == self.buf.len() { 0 } else { self.pos + 1 };
        y
    }
}

pub struct Sanctis {
    params: Arc<SanctisParams>,
    editor_state: Arc<EguiEditorState>,
    meters: Arc<Meters>,
    repaint: RepaintNotifier,
    initial_editor: Option<editor::SanctisEditor>,

    engine: Option<Engine>,
    /// What the engine was last set to. Compared every block.
    core: Option<CoreParams>,
    dry: Vec<[f32; 2]>,
    dry_delay: DryDelay,
    wave: wave::WaveMeter,
    sample_rate: f32,
}

impl Default for Sanctis {
    fn default() -> Self {
        let params = Arc::new(SanctisParams::default());
        let meters = Arc::new(Meters::default());
        Self {
            initial_editor: Some(editor::SanctisEditor::new(params.clone(), meters.clone())),
            editor_state: EguiEditorState::from_size(editor::WINDOW_SIZE, 1.0),
            params,
            meters,
            repaint: RepaintNotifier::new(),
            engine: None,
            core: None,
            dry: Vec::new(),
            dry_delay: DryDelay::default(),
            wave: wave::WaveMeter::default(),
            sample_rate: 48000.0,
        }
    }
}

impl Sanctis {
    /// The engine parameters for the current knob positions. Input and output
    /// gain stay in the wrapper (they are smoothed per sample); the split and
    /// the limiter ramps are fixed, so the latency never changes.
    fn core_params(p: &SanctisParams) -> CoreParams {
        let d = CoreParams::reference();
        CoreParams {
            character_enabled: p.character.value(),
            bands: [p.low.band_params(), p.mid.band_params(), p.high.band_params()],
            limiter: LimiterParams {
                mode: match p.stage.value() {
                    StageMode::Off => OutputMode::Off,
                    StageMode::Limit => OutputMode::Limit,
                    StageMode::Clip => OutputMode::Clip,
                },
                input_gain_db: p.lim_drive.value(),
                ceiling_db: p.lim_ceiling.value(),
                slow_attack_ms: p.lim_attack.value(),
                slow_release_ms: p.lim_release.value(),
                ..d.limiter
            },
            shaper_oversample: if p.quality.value() == Quality::X2 { 2 } else { 1 },
            ..d
        }
    }
}

impl Plugin for Sanctis {
    const NAME: &'static str = "SACRIA C-SANCTIS";
    const VENDOR: &'static str = "anselic";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The detectors listen to the mono sum and the band gains are M/S: needs a stereo pair.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<editor::SanctisEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_egui_editor(
            self.editor_state.clone(),
            self.repaint.clone(),
            EguiNiceSettings::new().with_resize_hint(editor::RESIZE_HINT),
            self.initial_editor.take()?,
        )
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        // Allocation is fine here; `process` never allocates.
        let core = Self::core_params(&self.params);
        let engine = Engine::new(buffer_config.sample_rate, core);
        let latency = engine.latency_samples();
        context.set_latency_samples(latency as u32);
        self.dry = vec![[0.0; 2]; buffer_config.max_buffer_size as usize];
        self.dry_delay.resize(latency);
        self.sample_rate = buffer_config.sample_rate;
        self.wave.set_sample_rate(buffer_config.sample_rate);
        self.meters.spectrum.sample_rate.store(buffer_config.sample_rate, Ordering::Relaxed);
        self.engine = Some(engine);
        self.core = Some(core);
        true
    }

    fn reset(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            e.reset();
        }
        self.dry_delay.clear();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let core = Self::core_params(&self.params);
        let Some(engine) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };
        if self.core != Some(core) {
            engine.set_params(&core);
            self.core = Some(core);
        }

        let n = buffer.samples();
        let channels = buffer.as_slice();
        let (a, b) = channels.split_at_mut(1);
        let (left, right) = (&mut *a[0], &mut *b[0]);

        for i in 0..n {
            self.dry[i] = [left[i], right[i]];
            let g = self.params.input.smoothed.next();
            left[i] *= g;
            right[i] *= g;
        }

        engine.process(&mut left[..n], &mut right[..n]);

        for i in 0..n {
            let dry = self.dry_delay.tick(self.dry[i]);
            let mix = self.params.mix.smoothed.next();
            let out = self.params.output.smoothed.next();
            left[i] = (mix * left[i] + (1.0 - mix) * dry[0]) * out;
            right[i] = (mix * right[i] + (1.0 - mix) * dry[1]) * out;
        }
        if self.editor_state.is_open() {
            let (gr, level) = (engine.band_gain_reduction_db(), engine.band_level_db());
            for k in 0..NUM_BANDS {
                self.meters.gr[k].store(gr[k], Ordering::Relaxed);
                self.meters.level[k].store(level[k], Ordering::Relaxed);
            }
            self.meters.limiter.store(engine.limiter_gain_reduction_db(), Ordering::Relaxed);
            self.meters.spectrum.write(&left[..n], &right[..n]);
            let t = context.transport();
            let column_len = wave::column_len(self.sample_rate, t.tempo, t.time_sig_numerator, t.time_sig_denominator);
            self.wave.process(&left[..n], &right[..n], column_len, t.playing, &self.meters.wave);
            self.repaint.request_repaint();
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Sanctis {
    const CLAP_ID: &'static str = "com.anselic.c-sanctis";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("3-band mastering glue: compression, saturation, width, limiter");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Compressor,
        ClapFeature::Limiter,
        ClapFeature::Mastering,
    ];
}

impl Vst3Plugin for Sanctis {
    const VST3_CLASS_ID: [u8; 16] = *b"AnselicSanctis__";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics, Vst3SubCategory::Mastering];
}

nice_export_clap!(Sanctis);
nice_export_vst3!(Sanctis);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_presets_are_valid() {
        sacria_ui::presets::check_factory(&SanctisParams::default(), FACTORY).unwrap();
    }
}
