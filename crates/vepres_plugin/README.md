# C-VEPRES 0.1.0-beta.1

Windows, 64-bit. VST3 and CLAP.

An opinionated M/S compressor: 3 bands each for mid and side, a soft clipper and
a true-peak limiter. The sound is mostly fixed; a few knobs move it from there.

**BETA.** Please do not share it further yet.

## Install

| Format | Copy                      | To                                   |
| ------ | ------------------------- | ------------------------------------ |
| VST3   | `C-VEPRES.vst3`           | `C:\Program Files\Common Files\VST3\` |
| CLAP   | `C-VEPRES.clap`           | `C:\Program Files\Common Files\CLAP\` |

Then rescan plugins in your DAW.

## Use

Stereo tracks or buses only (it works in mid/side). At the default knob
settings you get the tuned sound. The knobs move it from there:

| Control       | What it does                                                          |
| ------------- | --------------------------------------------------------------------- |
| 01 INPUT      | Gain into the processor (±12 dB). Drives the compressor and clipper.  |
| 02 THRESHOLD  | Added to every band threshold (±12 dB). Up = less compression.        |
| 03 WIDTH      | Side level after compression (±6 dB).                                 |
| 04 OUTPUT     | Gain after everything (±12 dB).                                       |
| 05 MIX        | Dry/wet. The dry signal is delay-compensated.                         |
| 06 LIMITER    | True-peak limiter at the end, about -1 dBTP. On by default.           |
| 07 CLIP       | Soft clipper before the bands. `OFF`, `ON` (default) or `2X`.         |

- **Clip modes:** `ON` only touches peaks near 0 dBFS. `2X` is the same
  clipper at twice the sample rate, so hot peaks do not fold back as harsh
  tones. It costs a little more CPU. `OFF` skips the clipper.
- **Knobs:** drag up/down, hold Shift for fine moves, double-click to reset.
- **Help:** hover a control to read what it does at the bottom of the window.
- **Window:** resizable; drag the corner.
- **Latency:** about 11 ms at 48 kHz. Your DAW compensates for it automatically.

## Known limits

- Checked at 48 kHz. 44.1, 88.2 and 96 kHz were checked and sound the same.
- Very loud bass in the side channel can come out about 0.4 dB louder than
  intended.
- The meters show gain reduction from 0 to -12 dB. Deeper reduction still
  works, it just pins the meter.

## Feedback

Useful to hear:

- how it sounds on your material, next to what you normally use
- CPU use, with `CLIP` on `ON` and on `2X`
- crashes, clicks or glitches
- your DAW + version, sample rate and buffer size

## Credits

GUI font: Geist Mono, SIL Open Font License 1.1 (see `GeistMono-OFL.txt`).
