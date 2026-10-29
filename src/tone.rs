//! Tones for the speaker: a wave at a frequency, for a time, as the samples
//! [`Speaker::run`] plays.
//!
//! [`Speaker::run`]: crate::Speaker::run

use core::time::Duration;

use crate::units::{Hertz, Percent};

/// The speaker's sample rate (and the microphone's).
pub const SAMPLE_RATE: u32 = 44_100;

/// The samples a tone fades in and out over (2 ms), so it starts and stops
/// without a click.
const FADE: u32 = SAMPLE_RATE / 500;

/// A tone's wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Waveform {
    /// A square wave: the bright beep of old games and alarms.
    #[default]
    Square,
    /// A triangle wave: softer, closer to a whistle.
    Triangle,
}

/// A tone: a [`Waveform`] at a frequency, for a time, as samples at
/// [`SAMPLE_RATE`].
///
/// As an iterator it gives the tone's samples, then ends;
/// [`fill`](Self::fill) writes them, then silence, for [`Speaker::run`]:
///
/// ```ignore
/// let mut beep = Tone::new(Hertz(880), Duration::from_millis(100));
/// speaker.run(|samples| beep.fill(samples)).await
/// ```
///
/// [`Speaker::run`]: crate::Speaker::run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tone {
    waveform: Waveform,
    /// The peak sample.
    amplitude: i32,
    /// How far through a cycle each sample moves, out of 2³².
    step: u32,
    phase: u32,
    /// Samples in all, and still to come.
    total: u32,
    left: u32,
}

impl Tone {
    /// A square wave at `frequency` for `duration`, at half the full level.
    /// Frequencies above half the sample rate (22 kHz) alias.
    #[must_use]
    pub fn new(frequency: Hertz, duration: Duration) -> Self {
        let samples = duration.as_micros() * u128::from(SAMPLE_RATE) / 1_000_000;
        let total = u32::try_from(samples).unwrap_or(u32::MAX);
        // Rounded to the nearest, so the cycles land on time.
        let rate = u64::from(SAMPLE_RATE);
        let step = ((u64::from(frequency.0) << 32) + rate / 2) / rate;
        Self {
            waveform: Waveform::Square,
            amplitude: 0,
            step: u32::try_from(step).unwrap_or(u32::MAX),
            phase: 0,
            total,
            left: total,
        }
        .with_level(Percent::new(50))
    }

    /// The same tone with another wave.
    #[must_use]
    pub const fn with_waveform(self, waveform: Waveform) -> Self {
        Self { waveform, ..self }
    }

    /// The same tone at `level` of the samples' full scale. The codec's
    /// volume ([`Codec::set_volume`]) comes on top of it.
    ///
    /// [`Codec::set_volume`]: crate::Codec::set_volume
    #[must_use]
    pub fn with_level(self, level: Percent) -> Self {
        let amplitude = i32::from(i16::MAX) * i32::from(level.get()) / 100;
        Self { amplitude, ..self }
    }

    /// Whether it's all been played.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.left == 0
    }

    /// Writes the next of its samples to `samples`, and silence once it's
    /// done.
    pub fn fill(&mut self, samples: &mut [i16]) {
        for sample in samples {
            *sample = self.next().unwrap_or(0);
        }
    }
}

impl Iterator for Tone {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let played = self.total - self.left;
        self.left = self.left.checked_sub(1)?;
        // Where in the cycle, from -32768 to 32767.
        let wave = match self.waveform {
            Waveform::Square if self.phase < 1 << 31 => 32767,
            Waveform::Square => -32768,
            Waveform::Triangle => {
                let x = i32::try_from(self.phase >> 15).unwrap_or(0);
                if x < 1 << 16 { x - 32768 } else { 98303 - x }
            }
        };
        self.phase = self.phase.wrapping_add(self.step);
        // Up over the first samples, and down over the last.
        let fade = i32::try_from(played.min(self.left + 1).min(FADE)).unwrap_or(0);
        let sample = wave * self.amplitude / 32767 * fade / i32::try_from(FADE).unwrap_or(1);
        Some(i16::try_from(sample).unwrap_or(0))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = usize::try_from(self.left).unwrap_or(usize::MAX);
        (left, Some(left))
    }
}

impl ExactSizeIterator for Tone {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 441 Hz: a cycle is exactly 100 samples.
    fn tone(waveform: Waveform) -> Vec<i16> {
        Tone::new(Hertz(441), Duration::from_secs(1))
            .with_waveform(waveform)
            .with_level(Percent::FULL)
            .collect()
    }

    #[test]
    fn a_tone_lasts_its_duration() {
        let tone = Tone::new(Hertz(440), Duration::from_millis(10));
        assert_eq!(tone.len(), 441);
        assert_eq!(tone.count(), 441);
    }

    #[test]
    fn a_square_wave_is_high_for_half_a_cycle_then_low() {
        let samples = tone(Waveform::Square);
        // Past the fade in.
        assert!(samples[200..250].iter().all(|&sample| sample == 32767));
        assert!(samples[250..300].iter().all(|&sample| sample == -32768));
    }

    #[test]
    fn a_triangle_wave_rises_for_half_a_cycle_then_falls() {
        let samples = tone(Waveform::Triangle);
        assert_eq!(samples[200], -32768);
        assert!(samples[200..250].is_sorted());
        assert!(samples[250] > 32000);
        assert!(samples[250..300].iter().rev().is_sorted());
    }

    #[test]
    fn it_fades_in_and_out() {
        let samples = tone(Waveform::Square);
        assert_eq!(samples[0], 0);
        assert!(samples[1].unsigned_abs() < 1000);
        assert!(
            samples
                .last()
                .is_some_and(|sample| sample.unsigned_abs() < 1000)
        );
    }

    #[test]
    fn its_level_scales_it() {
        let samples: Vec<i16> = Tone::new(Hertz(441), Duration::from_secs(1))
            .with_level(Percent::new(50))
            .collect();
        assert_eq!(samples[200], 16383);
    }

    #[test]
    fn filling_goes_on_with_silence_once_its_done() {
        let mut tone = Tone::new(Hertz(441), Duration::from_millis(1));
        let mut samples = [1; 64];
        tone.fill(&mut samples);
        assert!(tone.is_done());
        assert!(samples[44..].iter().all(|&sample| sample == 0));
    }
}
