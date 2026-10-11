# Sacria
Sacria series plugins made under Anselic, made in Rust with [`nice-plug`](https://codeberg.org/RustAudio/nice-plug).

The goal for Sacria is to provide free set of plugins for music production, while being fast and efficient on the CPU.

There will be builds for VST3 and CLAP. Will be tested on Windows on Ableton Live 12.

> [!NOTE]
> Eventually, most *DSP engines* will be rewrite in [haven](https://github.com/anselic/haven), but that is non-goal for now. Sacria will be a Rust project for the time being. Plugin wrappers will still be written in Rust.

> [!WARNING]
> This is a beta version. Use at your own risk and expect breaking changes.

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
- Clone the repo
- You can either:
    - Using `bundle.py` script:

        Run the [`bundle.py`](./bundle.py) script to build all plugins and bundle them
        ```shell
        # list plugins available to bundle
        $ python3 bundle.py list
        vepres           C-VEPRES
        sanctis          C-SANCTIS
        # build and bundle all plugins
        $ python3 bundle.py --all
        # build and bundle a specific plugin(s)
        $ python3 bundle.py sanctis vepres
        # just bundle without building
        $ python3 bundle.py sanctis vepres --no-build
        # debug build for faster build time
        $ python3 bundle.py sanctis vepres --fast

        # example output
        Created a standalone bundle at '.../C-SANCTIS.exe'
        Created a CLAP bundle at '...\C-SANCTIS.clap'
        Created a VST3 bundle at '...\C-SANCTIS.vst3'

        Created a standalone bundle at '.../C-VEPRES.exe'
        Created a CLAP bundle at '...\C-VEPRES.clap'
        Created a VST3 bundle at '...\C-VEPRES.vst3'
        wrote dist\C-SANCTIS-0.1.0-beta.1-win64.zip
        wrote dist\C-VEPRES-0.1.0-beta.1-win64.zip
        ```
        Then copy the .vst3/.clap from `target/bundled` to your plugin folder, or pass `--copy [PATH]` (optional path) to copy VST3 plugin(s) to default/specific plugin folder. (Default is `C:\Program Files\Common Files\VST3`)
    - With `xtask`:
        ```shell
        $ cargo xtask bundle <plugin name> [--release] [--profile fast]
        ```
        Then copy the `.vst3`/`.clap` from `target/bundled` to your plugin folder.

    Please note that full release build will take a long time to build, so for iteration and testing pass `--profile fast` to build a debug build instead.


# License
Code is licensed under [GPLv3](./LICENSE).

The presets in this repo are released under CC0 (public domain).

Geist fonts are licensed under [SIL Open Font License 1.1](./assets//fonts/GeistMono-OFL.txt).

Image assets (logo, icons) are licensed under [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/).
