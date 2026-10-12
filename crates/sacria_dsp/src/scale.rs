//! Keys and scales for the color modules.

pub const KEY_NAMES: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const SCALE_NAMES: &[&str] =
    &["MINOR", "MAJOR", "MIN PENTA", "MIN CHORD", "MAJ CHORD", "MIN7", "FIFTH", "OCTAVE", "CHROMATIC"];
/// Pitch classes of each scale, as bits (bit 0 = root).
const MASKS: [u16; 9] = [
    0b0101_1010_1101,
    0b1010_1011_0101,
    0b0100_1010_1001,
    0b0000_1000_1001,
    0b0000_1001_0001,
    0b0100_1000_1001,
    0b0000_1000_0001,
    0b0000_0000_0001,
    0b1111_1111_1111,
];

#[derive(Clone, Copy, PartialEq)]
pub struct Scale {
    key: f32,
    mask: u16,
}

impl Scale {
    pub fn new(key: f32, scale: f32) -> Self {
        Self { key: key.round(), mask: MASKS[(scale.round() as usize).min(MASKS.len() - 1)] }
    }

    /// 1 for every pitch class in the scale, 0 for the rest (index 0 = C).
    pub fn weights(&self) -> [f32; 12] {
        let mut w = [0.0; 12];
        for step in 0..12 {
            if self.mask >> step & 1 == 1 {
                w[(self.key as usize + step) % 12] = 1.0;
            }
        }
        w
    }

    /// Nearest scale note to a MIDI note number (fractional in, whole note out).
    pub fn nearest(&self, midi: f32) -> f32 {
        let rel = midi - self.key;
        let base = (rel / 12.0).floor() * 12.0;
        let pc = rel - base;
        let mut best = 0.0f32;
        let mut dist = f32::MAX;
        // one octave plus the neighbours on both sides
        for step in -12i32..24 {
            if self.mask >> step.rem_euclid(12) & 1 == 0 {
                continue;
            }
            let d = (step as f32 - pc).abs();
            if d < dist {
                dist = d;
                best = step as f32;
            }
        }
        base + best + self.key
    }
}

#[inline]
pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

#[inline]
pub fn midi_to_hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_the_scale() {
        // D# minor chord: D#, F#, A#
        let s = Scale::new(3.0, 3.0);
        assert_eq!(s.nearest(63.2), 63.0);
        assert_eq!(s.nearest(64.4), 63.0);
        assert_eq!(s.nearest(65.0), 66.0);
        assert_eq!(s.nearest(68.9), 70.0);
        assert_eq!(s.nearest(73.9), 75.0);
        assert_eq!(s.nearest(27.3), 27.0);
        assert_eq!(Scale::new(0.0, 8.0).nearest(61.4), 61.0);
    }
}
