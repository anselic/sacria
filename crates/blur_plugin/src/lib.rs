//! Plugin wrapper around `blur_engine::Engine`.
//!
//! Every control maps straight to one engine parameter. MIX (against the dry signal, delayed
//! by the engine latency) and OUT stay in the wrapper.

use blur_engine::{Engine, Params as CoreParams};
use nice_plug::prelude::*;
use nice_plug_egui::{create_egui_editor, EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};
use std::sync::{Arc, RwLock};

mod editor;

/// Built into the plugin: only the starting point. Every other preset is a file in the
/// user's preset folder.
pub const FACTORY: &[sacria_ui::presets::Factory] = &[sacria_ui::presets::Factory { name: sacria_ui::presets::DEFAULT_NAME, category: "", params: "{}" }];

#[derive(Params)]
pub struct BlurParams {
    /// Name of the loaded preset, for the preset bar.
    #[persist = "preset"]
    pub preset: Arc<RwLock<String>>,

    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "scatter"]
    pub scatter: FloatParam,
    /// How much of the input stays in the output. 0% = a null test.
    #[id = "source"]
    pub source: FloatParam,
    #[id = "freeze"]
    pub freeze: BoolParam,

    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
}

fn ms_param(name: &str, default: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Skewed { min: 0.0, max, factor: FloatRange::skew_factor(-2.0) })
        .with_step_size(1.0)
        .with_unit(" ms")
}

fn percent_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 100.0 }).with_step_size(1.0).with_unit(" %")
}

impl Default for BlurParams {
    fn default() -> Self {
        let d = CoreParams::default();
        Self {
            preset: Arc::new(RwLock::new(sacria_ui::presets::DEFAULT_NAME.to_owned())),
            attack: ms_param("Attack", d.attack_ms, 2000.0),
            decay: ms_param("Decay", d.decay_ms, 10000.0),
            scatter: percent_param("Scatter", d.scatter * 100.0),
            source: percent_param("Source", d.source * 100.0),
            freeze: BoolParam::new("Freeze", d.freeze),
            mix: percent_param("Mix", 100.0),
            output: FloatParam::new("Output", 0.0, FloatRange::Linear { min: -24.0, max: 12.0 }).with_step_size(0.1).with_unit(" dB"),
        }
    }
}

pub struct Blur {
    params: Arc<BlurParams>,
    editor_state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    initial_editor: Option<editor::BlurEditor>,

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

impl Default for Blur {
    fn default() -> Self {
        let params = Arc::new(BlurParams::default());
        Self {
            initial_editor: Some(editor::BlurEditor::new(params.clone())),
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

impl Blur {
    /// The engine parameters for the current knob positions.
    fn core_params(p: &BlurParams) -> CoreParams {
        CoreParams {
            attack_ms: p.attack.value(),
            decay_ms: p.decay.value(),
            scatter: p.scatter.value() / 100.0,
            source: p.source.value() / 100.0,
            freeze: p.freeze.value(),
        }
    }
}

impl Plugin for Blur {
    const NAME: &'static str = "SACRIA S-BLUR";
    const VENDOR: &'static str = "anselic";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<editor::BlurEditor>;
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

impl ClapPlugin for Blur {
    const CLAP_ID: &'static str = "com.anselic.s-blur";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Spectral smear: every partial gets its own attack and decay");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for Blur {
    const VST3_CLASS_ID: [u8; 16] = *b"AnselicBlur_____";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nice_export_clap!(Blur);
nice_export_vst3!(Blur);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_presets_are_valid() {
        sacria_ui::presets::check_factory(&BlurParams::default(), FACTORY).unwrap();
    }
}
