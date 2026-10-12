# Changelog

## [0.1.0-beta.2]

### Added
- **S-BLUR**: per-partial attack and decay effect.
- **S-PRISM**: key/scale spectral filter effect.

### Changed
- **Presets moved** to `Documents/Anselic/Sacria/<plugin>/Presets/` (was `Documents/Anselic/<plugin>/Presets/`). Move your old preset folders there by hand, or the plugin will not find them.
- Plugins now show up as "SACRIA <plugin name>" in the host’s plugin list. Instances in old projects should still load and may keep their old name.
- Use global version instead of per-plugin version.

### Internal
- Local release builds are now thin LTO, CI builds will still be full LTO. Also added caching to speed it up more.

## [0.1.0-beta.1]

First public beta of the Sacria series. Windows 64-bit, VST3 and CLAP.

### Added
- **C-VEPRES** `0.1.0-beta.1`: mid/side 3-band compressor for gluing groups together.
- **C-SANCTIS** `0.1.0-beta.1`: mastering multiband compressor, saturator and limiter for the master bus.

This is a beta. Things can change between versions, and presets may not carry over. Please report bugs in the Issues tab.