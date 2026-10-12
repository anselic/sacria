# C-SANCTIS

Windows, 64-bit. VST3 and CLAP.

A 3-band mastering glue: per band a gentle compressor, a sine shaper and
mid/side gains, then a true-peak limiter or a hard clipper. At the default settings you get the
tuned sound; every part of it has its own control.

**BETA.** Please do not share it further yet.

## Install

| Format | Copy                      | To                                    |
| ------ | ------------------------- | ------------------------------------- |
| VST3   | `C-SANCTIS.vst3`          | `C:\Program Files\Common Files\VST3\` |
| CLAP   | `C-SANCTIS.clap`          | `C:\Program Files\Common Files\CLAP\` |

Then rescan plugins in your DAW.

Presets (optional): copy what is inside the `Presets` folder into
`Documents\Anselic\Sacria\C-SANCTIS\Presets\` (make the folders if they are not
there). Keep the sub-folders: each one shows up as a category.

## Use

Stereo tracks or buses only (the compressors listen to the mono sum and the
band gains work in mid/side).

The signal is split into three bands: low (below 160 Hz), mid, and high (above
2.5 kHz). To keep the panel calm, the controls are on three pages. The **KNOB PAGE**
switch (above the output panel) flips all panels at once. It only changes what you see: every
knob keeps working and can be automated from either page.

Each band has the same nine knobs and two switches:

| Page | Control    | What it does                                                               |
| ---- | ---------- | -------------------------------------------------------------------------- |
| TONE | THRESH     | Level where the band's compressor starts. Lower = more compression.        |
| TONE | DRIVE      | Saturation (sine shaper, odd harmonics). 0 = clean.                        |
| TONE | MID        | Level of the band's mid signal (L+R), ±12 dB.                              |
| TONE | SIDE       | Level of the band's side signal (L-R), ±12 dB. More side than mid = wider. |
| TIME | RATIO      | How hard it compresses above the threshold. 1:1 = off.                     |
| TIME | KNEE       | Width of the soft transition around the threshold.                         |
| TIME | ATTACK     | How fast the compressor reacts to a level rise.                            |
| TIME | RELEASE    | How fast it lets go.                                                       |
| MODE | BLEND      | Parallel compression. 100% = compressed only; lower adds the dry band back. |
| MODE | SHAPE      | `SINE` (default): hot peaks fold back. `SOFT`: hot peaks are held flat.    |
| MODE | DETECT     | `MID` (default): compressor hears L+R only. `STEREO`: louder of L and R.   |

Output panel:

| Page | Control    | What it does                                                               |
| ---- | ---------- | -------------------------------------------------------------------------- |
| all  | OUTPUT STAGE | What catches the peaks: `OFF`, `LIM` (default) or `CLIP`. See below.     |
| all  | CHARACTER  | The three band panels. Off = the bands pass untouched, same latency.       |
| TONE | INPUT      | Gain into the processor (±12 dB).                                          |
| TONE | DRIVE      | Gain into the output stage (0 to 12 dB). More = louder.                    |
| TONE | OUTPUT     | Gain after the output stage (±12 dB).                                      |
| TONE | MIX        | Dry/wet. The dry signal is delay-compensated.                              |
| TIME | CEILING    | The level the output stage never passes (-6 to 0 dB).                      |
| TIME | LIM ATT    | `LIM` only: how fast the limiter's slow stage builds up.                   |
| TIME | LIM REL    | `LIM` only: how fast the limiter's slow stage lets go.                     |
| MODE | QUALITY    | `1X` (default) or `2X`: saturation at twice the sample rate.               |

- **Output stage:** `LIM` is the true-peak limiter: clean, but it pumps when
  you push it hard. `CLIP` is a hard clipper: peaks above the ceiling are cut
  flat, so there is no pumping, but more grit, and peaks between samples can
  read a little above the ceiling on a true-peak meter. `OFF` passes the signal
  untouched (no drive, no ceiling), for when you limit or clip later.
- **Shape and detect** are per band. `SOFT` and `STEREO` are not in the classic
  sound; they are there for when you want cleaner peaks or tighter wide content.
- **Quality:** `2X` costs more CPU and adds a little phase shift in the highs,
  so with MIX below 100% the top end can thin out slightly.
- **Scopes:** each band panel draws its real transfer curve (level in against
  level out, compressor and shaper together). The dot is the band's signal level
  right now; the bar next to it is the gain reduction, which can recover much
  more slowly (see RELEASE).
- **Response strip:** the thick lines show what the band gains do to quiet
  signals, 20 Hz to 20 kHz. The thin lines behind them are the spectrum of the
  output, tilted 4.5 dB per octave so a balanced mix looks flat. Solid = mid,
  dashed = side. Where side sits above mid, the sound gets wider.
- **Waveform:** click the response strip to switch it to a colored waveform of
  the newest output, and click again to go back. Scroll (or Ctrl + scroll) over
  it to zoom from 1/16 bar to 32 bars; it opens on 4 bars. It is modeled on a DJ
  software waveform: red = lows (below about 190 Hz), green = mids, blue = highs
  (above about 1.1 kHz). The color shows which has the most power, the height is
  power (6 dB down = a quarter of the height). If your drop is mostly red, it
  has a lot of low end. It follows the host tempo (120 BPM if there is none) and
  stops scrolling when the host is stopped and silent.
- **Output meter:** the bar in the output panel shows how much the limiter
  turns the signal down, or in `CLIP` mode how far the peaks are over the
  ceiling, 0 to -12 dB.
- **Hot signals:** with high drive and `SHAPE` on `SINE`, peaks fold back
  instead of clipping. You can see it as the curve bending down at the top
  right of a scope. `SOFT` holds them flat instead.
- **Presets:** the bar at the top. Click the name for the list, use the arrows
  to step, and the disk to save the current settings under a name (saving over
  an existing preset replaces it). Presets are plain files in
  `Documents\Anselic\Sacria\C-SANCTIS\Presets\`; a folder in there is a category.
  `Default` is built in and cannot be replaced. A star after the name means a
  knob moved since the preset was loaded.
- **Knobs:** drag up/down, hold Shift for fine moves, double-click to reset.
- **Help:** hover a control to read what it does at the bottom of the window.
- **Window:** resizable; drag the corner.
- **Latency:** about 7.6 ms at 48 kHz. Your DAW compensates for it automatically.

## Feedback

Useful to hear:

- how it sounds on your material, next to what you normally use
- CPU use, with `QUALITY` on `1X` and on `2X`
- crashes, clicks or glitches
- your DAW + version, sample rate and buffer size

## Credits

GUI font: Geist Mono, SIL Open Font License 1.1 (see `GeistMono-OFL.txt`).
