//! Plugin wrapper around `vepres_engine::Engine`.
//!
//! The knobs here are offsets on top of the core's default voicing
//! (`Params::up_and_wide()`); at their defaults the plugin is that voicing.

use vepres_engine::{Engine, Params as CoreParams};
use atomic_float::AtomicF32;
use nice_plug::prelude::*;
use nice_plug_egui::{create_egui_editor, EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

mod editor;

/// Gain reduction for the GUI: mid low/mid/high, side low/mid/high, limiter (dB, <= 0).
pub struct Meters {
    pub gr: [AtomicF32; 7],
}

impl Default for Meters {
    fn default() -> Self {
        Self { gr: std::array::from_fn(|_| AtomicF32::new(0.0)) }
    }
}

/// Built into the plugin: only the starting point. Every other preset is a file in the
/// user's preset folder.
pub const FACTORY: &[sacria_ui::presets::Factory] = &[sacria_ui::presets::Factory { name: sacria_ui::presets::DEFAULT_NAME, category: "", params: "{}" }];

#[derive(Params)]
pub struct VepresParams {
    /// Name of the loaded preset, for the preset bar.
    #[persist = "preset"]
    pub preset: Arc<RwLock<String>>,

    #[id = "input"]
    pub input: FloatParam,
    /// Added to every band threshold. Positive = less compression.
    #[id = "thresh"]
    pub threshold: FloatParam,
    /// Added to the side band makeups.
    #[id = "width"]
    pub width: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "limiter"]
    pub limiter: BoolParam,
    /// Soft clipper: off, on, or on at 2x (less aliasing, about 0.6% more of a core).
    #[id = "clip"]
    pub clip: EnumParam<ClipMode>,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "on"]
    #[name = "On"]
    On,
    #[id = "2x"]
    #[name = "2x"]
    X2,
}

impl Default for VepresParams {
    fn default() -> Self {
        let gain = |name: &str| {
            FloatParam::new(name, 1.0, FloatRange::gain_range(-12.0, 12.0))
                .with_smoother(SmoothingStyle::Logarithmic(30.0))
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
                .with_string_to_value(formatters::s2v_f32_gain_to_db())
        };
        let offset = |name: &str, range: f32| {
            FloatParam::new(name, 0.0, FloatRange::Linear { min: -range, max: range })
                .with_unit(" dB")
                .with_step_size(0.1)
        };
        Self {
            preset: Arc::new(RwLock::new(sacria_ui::presets::DEFAULT_NAME.to_owned())),
            input: gain("Input"),
            threshold: offset("Threshold", 12.0),
            width: offset("Width", 6.0),
            output: gain("Output"),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(30.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            limiter: BoolParam::new("Limiter", true),
            clip: EnumParam::new("Clip", ClipMode::On),
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

/// The knob values that need an `Engine::set_params`. Compared every block.
#[derive(Clone, Copy, PartialEq)]
struct EngineKnobs {
    threshold: f32,
    width: f32,
    limiter: bool,
    clip: ClipMode,
}

pub struct Vepres {
    params: Arc<VepresParams>,
    editor_state: Arc<EguiEditorState>,
    meters: Arc<Meters>,
    repaint: RepaintNotifier,
    initial_editor: Option<editor::VepresEditor>,

    engine: Option<Engine>,
    knobs: Option<EngineKnobs>,
    dry: Vec<[f32; 2]>,
    dry_delay: DryDelay,
}

impl Default for Vepres {
    fn default() -> Self {
        let params = Arc::new(VepresParams::default());
        let meters = Arc::new(Meters::default());
        Self {
            initial_editor: Some(editor::VepresEditor::new(params.clone(), meters.clone())),
            editor_state: EguiEditorState::from_size(editor::WINDOW_SIZE, 1.0),
            params,
            meters,
            repaint: RepaintNotifier::new(),
            engine: None,
            knobs: None,
            dry: Vec::new(),
            dry_delay: DryDelay::default(),
        }
    }
}

impl Vepres {
    fn core_params(k: EngineKnobs) -> CoreParams {
        let mut p = CoreParams::up_and_wide();
        for b in p.mid.bands.iter_mut().chain(p.side.bands.iter_mut()) {
            b.threshold_db += k.threshold;
        }
        // the low-band step trigger sits relative to the low band threshold
        p.mid.low_step.threshold_db += k.threshold;
        p.side.low_step.threshold_db += k.threshold;
        for b in p.side.bands.iter_mut() {
            b.makeup_db += k.width;
        }
        p.limiter.enabled = k.limiter;
        p.mid.clip.enabled &= k.clip != ClipMode::Off;
        p.side.clip.enabled &= k.clip != ClipMode::Off;
        p.clip_oversample = if k.clip == ClipMode::X2 { 2 } else { 1 };
        p
    }

    fn current_knobs(&self) -> EngineKnobs {
        EngineKnobs {
            threshold: self.params.threshold.value(),
            width: self.params.width.value(),
            limiter: self.params.limiter.value(),
            clip: self.params.clip.value(),
        }
    }
}

impl Plugin for Vepres {
    const NAME: &'static str = "C-VEPRES";
    const VENDOR: &'static str = "anselic";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // M/S processing needs a stereo pair.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<editor::VepresEditor>;
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
        let knobs = self.current_knobs();
        let engine = Engine::new(buffer_config.sample_rate, Self::core_params(knobs));
        let latency = engine.latency_samples();
        context.set_latency_samples(latency as u32);
        self.dry = vec![[0.0; 2]; buffer_config.max_buffer_size as usize];
        self.dry_delay.resize(latency);
        self.engine = Some(engine);
        self.knobs = Some(knobs);
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
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let knobs = self.current_knobs();
        let Some(engine) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };
        if self.knobs != Some(knobs) {
            engine.set_params(&Self::core_params(knobs));
            self.knobs = Some(knobs);
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
            let gr = engine.band_gain_reduction_db();
            for (k, v) in gr[0].iter().chain(gr[1].iter()).enumerate() {
                self.meters.gr[k].store(*v, Ordering::Relaxed);
            }
            self.meters.gr[6].store(engine.limiter_gain_reduction_db(), Ordering::Relaxed);
            self.repaint.request_repaint();
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Vepres {
    const CLAP_ID: &'static str = "com.anselic.c-vepres";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("M/S 3-band compressor");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::AudioEffect, ClapFeature::Stereo, ClapFeature::Compressor, ClapFeature::Mastering];
}

impl Vst3Plugin for Vepres {
    const VST3_CLASS_ID: [u8; 16] = *b"AnselicVepres___";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nice_export_clap!(Vepres);
nice_export_vst3!(Vepres);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_presets_are_valid() {
        sacria_ui::presets::check_factory(&VepresParams::default(), FACTORY).unwrap();
    }
}
