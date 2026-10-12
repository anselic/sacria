# S-BLUR

Windows, 64-bit. VST3 and CLAP.

A spectral smear. Every partial of the sound gets its own attack and decay
time, so hits can swell in and tones can ring out long after the input stops.
It is a tail with no room sound in it.

**BETA.** Expect changes between versions.

## Install

| Format | Copy          | To                                    |
| ------ | ------------- | ------------------------------------- |
| VST3   | `S-BLUR.vst3` | `C:\Program Files\Common Files\VST3\` |
| CLAP   | `S-BLUR.clap` | `C:\Program Files\Common Files\CLAP\` |

Then rescan plugins in your DAW.

## Use

Stereo tracks or buses only.

| Control | What it does                                                                        |
| ------- | ----------------------------------------------------------------------------------- |
| ATTACK  | How slowly each partial fades in. 0 = hits stay sharp. Longer = the sound swells in. |
| DECAY   | How long each partial rings after the input lets go of it (time to fall 60 dB).     |
| SCATTER | Random phase on the ringing partials. Low = a clean, static tail. High = noise and wash. |
| SOURCE  | How much of the input stays in the output. 0% = only what the blur added or removed. |
| FREEZE  | Holds the tone as it is right now and ignores the input until it is turned off.     |
| MIX     | Dry/wet. The dry signal is delay-compensated.                                       |
| OUT     | Gain at the end (-24 to +12 dB).                                                    |

- **Ringing partials keep their pitch:** a partial that rings on stays at the
  frequency it had when the input let go of it.
- **Null test:** set SOURCE to 0% and MIX to 100% to hear only the difference
  between the effect and the input.
- **Presets:** the bar at the top. Click the name for the list, use the arrows
  to step, and the disk to save the current settings under a name (saving over
  an existing preset replaces it). Presets are plain files in
  `Documents\Anselic\Sacria\S-BLUR\Presets\`; a folder in there is a category.
  `Default` is built in and cannot be replaced. A star after the name means a
  knob moved since the preset was loaded.
- **Knobs:** drag up/down, hold Shift for fine moves, double-click to reset.
- **Help:** hover a control to read what it does at the bottom of the window.
- **Window:** resizable; drag the corner.
- **Latency:** about 43 ms (2048 samples at 44.1 and 48 kHz, 4096 above). Your
  DAW compensates for it automatically.

## Feedback

Useful to hear:

- how it sounds on your material
- CPU use
- crashes, clicks or glitches
- your DAW + version, sample rate and buffer size

## Credits

GUI font: Geist Mono, SIL Open Font License 1.1 (see `GeistMono-OFL.txt`).
