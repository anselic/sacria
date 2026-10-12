# S-PRISM

Windows, 64-bit. VST3 and CLAP.

A key and scale spectral filter, or a "coloring" plugin. Only what sits on the
notes of a key and scale gets through, and it rings out at exactly those notes.
The rest is turned down. It works best on dense sounds (noise, distorted bass,
drums), because they have something on every note.

**BETA.** Expect changes between versions.

## Install

| Format | Copy           | To                                    |
| ------ | -------------- | ------------------------------------- |
| VST3   | `S-PRISM.vst3` | `C:\Program Files\Common Files\VST3\` |
| CLAP   | `S-PRISM.clap` | `C:\Program Files\Common Files\CLAP\` |

Then rescan plugins in your DAW.

## Use

Stereo tracks or buses only.

| Control    | What it does                                                                     |
| ---------- | -------------------------------------------------------------------------------- |
| KEY        | The root note of the scale.                                                      |
| SCALE      | Which notes are let through. Fewer notes = a stronger, more tuned color.         |
| AMOUNT     | How far everything between the notes is turned down. 100% = only the notes are left. |
| WIDTH      | How wide the gap around each note is, in cents. Narrow = pure, whistling tones.  |
| RING       | How long each note rings on after the input moves away (time to fall 60 dB).     |
| LOW KEEP   | Everything under this frequency passes untouched, so the sub and the kick stay solid. |
| SOURCE     | How much of the input stays in the output. 0% = only what the effect added or removed. |
| LEVEL LOCK | Brings the level back up to the input level after the mask (up to 18 dB).        |
| MIX        | Dry/wet. The dry signal is delay-compensated.                                    |
| OUT        | Gain at the end (-24 to +12 dB).                                                 |

- **Scales:** minor, major, minor pentatonic, minor chord, major chord, minor 7,
  root + fifth, octaves only, chromatic.
- **The ring is always in tune:** a note that rings on is set to the exact
  pitch of the scale note, even when the input was a little off.
- **Low notes are less exact:** the filter cannot be narrower than its own
  resolution (about 23 Hz at 48 kHz), so the color is loose in the low mids
  and gets precise higher up.
- **Null test:** set SOURCE to 0% and MIX to 100% to hear only the difference
  between the effect and the input.
- **Presets:** the bar at the top. Click the name for the list, use the arrows
  to step, and the disk to save the current settings under a name (saving over
  an existing preset replaces it). Presets are plain files in
  `Documents\Anselic\Sacria\S-PRISM\Presets\`; a folder in there is a category.
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
