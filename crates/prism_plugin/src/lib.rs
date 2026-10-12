//! Plugin wrapper around `prism_engine::Engine`.
//!
//! Every control maps straight to one engine parameter. MIX (against the dry signal, delayed
//! by the engine latency) and OUT stay in the wrapper.

use prism_engine::{Engine, Params as CoreParams, KEY_NAMES, SCALE_NAMES};
use nice_plug::prelude::*;
use nice_plug_egui::{create_egui_editor, EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};
use std::sync::{Arc, RwLock};

mod editor;

/// Built into the plugin: only the starting point. Every other preset is a file in the
/// user's preset folder.
pub const FACTORY: &[sacria_ui::presets::Factory] = &[sacria_ui::presets::Factory { name: sacria_ui::presets::DEFAULT_NAME, category: "", params: "{}" }];

#[derive(Params)]
pub struct PrismParams {
    /// Name of the loaded preset, for the preset bar.
    #[persist = "preset"]
    pub preset: Arc<RwLock<String>>,

    #[id = "key"]
    pub key: IntParam,
    #[id = "scale"]
    pub scale: IntParam,
    #[id = "amount"]
    pub amount: FloatParam,
    #[id = "width"]
    pub width: FloatParam,
    #[id = "ring"]
    pub ring: FloatParam,
    #[id = "low_keep"]
    pub low_keep: FloatParam,
    /// How much of the input stays in the output. 0% = a null test.
    #[id = "source"]
    pub source: FloatParam,
    #[id = "level_lock"]
    pub level_lock: BoolParam,

    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
}

/// A stepped parameter that shows one name per value.
fn names_param(name: &str, default: usize, names: &'static [&'static str]) -> IntParam {
    IntParam::new(name, default as i32, IntRange::Linear { min: 0, max: names.len() as i32 - 1 })
        .with_value_to_string(Arc::new(move |v| names[(v.max(0) as usize).min(names.len() - 1)].to_string()))
        .with_string_to_value(Arc::new(move |s| names.iter().position(|n| n.eq_ignore_ascii_case(s.trim())).map(|i| i as i32)))
}

fn log_param(name: &str, default: f32, min: f32, max: f32, unit: &'static str) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Skewed { min, max, factor: FloatRange::skew_factor(-2.0) })
        .with_step_size(1.0)
        .with_unit(unit)
}

fn percent_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 100.0 }).with_step_size(1.0).with_unit(" %")
}

impl Default for PrismParams {
    fn default() -> Self {
        let d = CoreParams::default();
        Self {
            preset: Arc::new(RwLock::new(sacria_ui::presets::DEFAULT_NAME.to_owned())),
            key: names_param("Key", d.key, KEY_NAMES),
            scale: names_param("Scale", d.scale, SCALE_NAMES),
            amount: percent_param("Amount", d.amount * 100.0),
            width: log_param("Width", d.width_ct, 5.0, 100.0, " ct"),
            ring: log_param("Ring", d.ring_ms, 0.0, 5000.0, " ms"),
            low_keep: log_param("Low Keep", d.low_keep_hz, 20.0, 1000.0, " Hz"),
            source: percent_param("Source", d.source * 100.0),
            level_lock: BoolParam::new("Level Lock", d.level_lock),
            mix: percent_param("Mix", 100.0),
            output: FloatParam::new("Output", 0.0, FloatRange::Linear { min: -24.0, max: 12.0 }).with_step_size(0.1).with_unit(" dB"),
        }
    }
}

pub struct Prism {
    params: Arc<PrismParams>,
    editor_state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    initial_editor: Option<editor::PrismEditor>,

    engine: Option<Engine>,
    /// What the engine was last set to. Compared every block.
    core: Option<CoreParams>,
    /// The dry signal, delayed by the engine latency.
    delay: [Vec<f32>; 2],
    delay_at: usize,
    /// Scratch copy of the input block.
    dry: [Vec<f32>; 2],
    /// Last block's wet and dry gains, for a click-free ramp.
    last: Option<(f32, f32)>,
}

impl Default for Prism {
    fn default() -> Self {
        let params = Arc::new(PrismParams::default());
        Self {
            initial_editor: Some(editor::PrismEditor::new(params.clone())),
            editor_state: EguiEditorState::from_size(editor::WINDOW_SIZE, 1.0),
            params,
            repaint: RepaintNotifier::new(),
            engine: None,
            core: None,
            delay: [Vec::new(), Vec::new()],
            delay_at: 0,
            dry: [Vec::new(), Vec::new()],
            last: None,
        }
    }
}

impl Prism {
    /// The engine parameters for the current knob positions.
    fn core_params(p: &PrismParams) -> CoreParams {
        CoreParams {
            key: p.key.value() as usize,
            scale: p.scale.value() as usize,
            amount: p.amount.value() / 100.0,
            width_ct: p.width.value(),
            ring_ms: p.ring.value(),
            low_keep_hz: p.low_keep.value(),
            source: p.source.value() / 100.0,
            level_lock: p.level_lock.value(),
        }
    }
}

impl Plugin for Prism {
    const NAME: &'static str = "SACRIA S-PRISM";
    const VENDOR: &'static str = "anselic";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<editor::PrismEditor>;
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
        self.delay = [vec![0.0; latency], vec![0.0; latency]];
        self.delay_at = 0;
        let block = (buffer_config.max_buffer_size as usize).max(64);
        self.dry = [vec![0.0; block], vec![0.0; block]];
        self.engine = Some(engine);
        self.core = Some(core);
        self.last = None;
        true
    }

    fn reset(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            e.reset();
        }
        self.delay.iter_mut().for_each(|d| d.fill(0.0));
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let core = Self::core_params(&self.params);
        let Some(engine) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };
        if self.core != Some(core) {
            engine.set_params(&core);
            self.core = Some(core);
        }
        let out = 10f32.powf(self.params.output.value() / 20.0);
        let mix = self.params.mix.value() / 100.0;
        let target = (mix * out, (1.0 - mix) * out);
        let from = self.last.unwrap_or(target);
        self.last = Some(target);

        let total = buffer.samples();
        let channels = buffer.as_slice();
        let (a, b) = channels.split_at_mut(1);
        let (l, r) = (&mut a[0][..total], &mut b[0][..total]);

        // in chunks, in case the host sends more than it promised
        let chunk = self.dry[0].len();
        let mut at = 0;
        while at < total {
            let end = (at + chunk).min(total);
            let n = end - at;
            self.dry[0][..n].copy_from_slice(&l[at..end]);
            self.dry[1][..n].copy_from_slice(&r[at..end]);
            engine.process(&mut l[at..end], &mut r[at..end]);

            let lat = self.delay[0].len();
            for i in 0..n {
                let t = (at + i + 1) as f32 / total as f32;
                let (wet, dry) = (from.0 + (target.0 - from.0) * t, from.1 + (target.1 - from.1) * t);
                for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                    let mut d = self.dry[ch][i];
                    if lat > 0 {
                        std::mem::swap(&mut d, &mut self.delay[ch][self.delay_at]);
                    }
                    buf[at + i] = buf[at + i] * wet + d * dry;
                }
                if lat > 0 {
                    self.delay_at = (self.delay_at + 1) % lat;
                }
            }
            at = end;
        }

        if self.editor_state.is_open() {
            self.repaint.request_repaint();
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Prism {
    const CLAP_ID: &'static str = "com.anselic.s-prism";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Key and scale spectral filter: only the scale notes pass, and they ring in tune");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for Prism {
    const VST3_CLASS_ID: [u8; 16] = *b"AnselicPrism____";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nice_export_clap!(Prism);
nice_export_vst3!(Prism);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_presets_are_valid() {
        sacria_ui::presets::check_factory(&PrismParams::default(), FACTORY).unwrap();
    }
}
