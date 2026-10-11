# Sacria
Sacria series plugins made under Anselic, made in Rust with [`nice-plug`](https://codeberg.org/RustAudio/nice-plug).

The goal for Sacria is to provide free set of plugins for music production, while being fast and efficient on the CPU.

There will be builds for VST3 and CLAP. Will be tested on Windows on Ableton Live 12.

> [!NOTE]
> Eventually, most *DSP engines* will be rewrite in [haven](https://github.com/anselic/haven), but that is non-goal for now. Sacria will be a Rust project for the time being. Plugin wrappers will still be written in Rust.

## List of plugins

- `C-VEPRES`: Opinionated M/S Compressor `v0.1.0-beta.1` [(engine)](./crates/vepres_engine/) [(plugin)](./crates/vepres_plugin/)

    A multiband mid/side compressor + limiter designed to glue group of sounds together (e.g. orchestra group, vocals, synths, etc.). Opinionated because I designed the sound I wanted to achieve, so it is not really a general purpose mid/side compressor. But there is still controls to nudge it.

- `C-SANCTIS`: Mastering Multiband Glue `v0.1.0-beta.1` [(engine)](./crates/sanctis_engine/) [(plugin)](./crates/sanctis_plugin/)

    A mastering multiband compressor + saturator + limiter + more designed be put on the master bus. Should give you a enough freedom to shape the master.

## List of modules
- [sacria_dsp](./crates/sacria_dsp/) - Shared DSP building blocks (biquad, crossover, soft clip, limiter, oversampling, fast math).
- [sacria_ui](./crates/sacria_ui/) - Shared UI kit (egui widgets, fonts, theme).
- [vepres_engine](./crates/vepres_engine/) - C-VEPRES's DSP engine.
- [sanctis_engine](./crates/sanctis_engine/) - C-SANCTIS's DSP engine.

# Build & Install

## Dependencies

- Rust and Cargo from [rustup.rs](https://rustup.rs/).

## Building
TODO

# License
Code is licensed under [GPLv3](./LICENSE).
The presets in this repo are released under CC0 (public domain).
Geist fonts are licensed under [SIL Open Font License 1.1](./assets//fonts/GeistMono-OFL.txt).
Image assets (logo, icons) are licensed under [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/).
