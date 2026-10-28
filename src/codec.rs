//! The ES8311 codec's controls, on the I2C bus: its speaker output and
//! microphone input.

use core::fmt;

use embedded_hal::i2c::I2c as _;
use embedded_hal_bus::i2c::CriticalSectionDevice;

use crate::{
    Decibels, Error, I2cBus, I2cDevice, Percent,
    convert::{microphone_gain_step, volume_register},
};

/// The ES8311's I2C address.
const ES8311: u8 = 0x18;

/// The ES8311's DAC volume register: 0x00 is −95.5 dB, 0xBF is 0 dB, in
/// 0.5 dB steps.
const ES8311_DAC_VOLUME: u8 = 0x32;

/// The ES8311's system registers for the DAC's power and its output to
/// the amplifier: what [`Codec::set_output`] switches.
const ES8311_DAC_POWER: u8 = 0x12;
const ES8311_OUTPUT: u8 = 0x13;

/// The ES8311's clock manager register, with the DAC's clocks on (as
/// [`codec`] leaves it), or the ADC's too.
const ES8311_CLOCKS: u8 = 0x01;
const CLOCKS_DAC: u8 = 0xB5;
const CLOCKS_DAC_ADC: u8 = 0xBF;

/// The ES8311's microphone preamp (PGA) register: [`MIC1`] selects the
/// microphone's input, and the low nibble is the gain, 0–10 for 0–30 dB.
const ES8311_ADC_PGA: u8 = 0x14;
const MIC1: u8 = 0x10;

/// The ES8311 codec, its DAC path to the amplifier on: what
/// [`codec`] returns. Holding one means the codec has been set up,
/// so its level and output can be changed.
pub struct Codec {
    device: I2cDevice,
    /// The microphone preamp's gain, in 3 dB steps (0–10).
    microphone_gain: u8,
}

impl Codec {
    /// Sets the speaker's level, from [`Percent::ZERO`] (silent, −95.5 dB)
    /// to [`Percent::FULL`] (0 dB, the level [`codec`] starts at), evenly in
    /// decibels.
    ///
    /// # Errors
    ///
    /// If the codec doesn't answer.
    pub fn set_volume(&mut self, volume: Percent) -> Result<(), Error> {
        self.write(&[[ES8311_DAC_VOLUME, volume_register(volume)]])
    }

    /// Turns the output to the amplifier off (`on = false`), or back on as
    /// [`codec`] left it: the DAC powered down and its path to the
    /// amplifier cut, or both restored.
    ///
    /// Turn it off before the I2S stream's clock stops (in a light sleep,
    /// say): the codec clocks itself from that stream, and the clock stopping
    /// and starting clicks in the speaker. Turn it back on once the stream
    /// runs again.
    ///
    /// # Errors
    ///
    /// If the codec doesn't answer.
    pub fn set_output(&mut self, on: bool) -> Result<(), Error> {
        // On: the DAC first, then the path; off the other way round.
        if on {
            self.write(&[[ES8311_DAC_POWER, 0x00], [ES8311_OUTPUT, 0x10]])
        } else {
            self.write(&[[ES8311_OUTPUT, 0x00], [ES8311_DAC_POWER, 0x02]])
        }
    }

    /// Turns the microphone (the codec's preamp and ADC) on, for
    /// [`Audio`]'s stream, or off. It starts at 0 dB of preamp gain
    /// ([`set_microphone_gain`](Self::set_microphone_gain)). The sequence is
    /// M5Unified's Cardputer ADV microphone enable, without the parts that
    /// would reset or power down the speaker's DAC.
    ///
    /// # Errors
    ///
    /// If the codec doesn't answer.
    ///
    /// [`Audio`]: crate::Audio
    pub fn set_microphone(&mut self, on: bool) -> Result<(), Error> {
        if on {
            self.write(&[
                [ES8311_CLOCKS, CLOCKS_DAC_ADC],
                [0x0A, 0x0C], // ADC serial port: I2S, 16-bit
                [0x0E, 0x02], // system: PGA and ADC modulator on
                [ES8311_ADC_PGA, MIC1 | self.microphone_gain],
                [0x17, 0xBF], // ADC volume: 0 dB
                [0x1C, 0x6A], // ADC equalizer bypassed, DC offset removed
            ])
        } else {
            // The PGA and the ADC off, then their clocks.
            self.write(&[[0x0E, 0x6A], [ES8311_CLOCKS, CLOCKS_DAC]])
        }
    }

    /// Sets the microphone preamp's gain, 0 to 30 dB in 3 dB steps (rounded
    /// down; more than 30 is 30). It's kept while the microphone is off.
    ///
    /// # Errors
    ///
    /// If the codec doesn't answer.
    pub fn set_microphone_gain(&mut self, gain: Decibels) -> Result<(), Error> {
        self.microphone_gain = microphone_gain_step(gain);
        self.write(&[[ES8311_ADC_PGA, MIC1 | self.microphone_gain]])
    }

    /// Writes each `[register, value]` in turn.
    fn write(&mut self, writes: &[[u8; 2]]) -> Result<(), Error> {
        writes
            .iter()
            .try_for_each(|write| self.device.write(ES8311, write).map_err(|_| Error::Codec))
    }
}

/// Turns on the ES8311's DAC path to the amplifier, at 0 dB, and returns
/// the [`Codec`] to change its level and output.
///
/// Its master clock comes from
/// the I2S bit clock (the board has no MCLK line), so the [`speaker`] stream
/// must be running. The sequence is M5Unified's Cardputer ADV speaker
/// enable, as espp's driver uses it.
///
/// # Errors
///
/// If the codec doesn't answer.
///
/// [`speaker`]: crate::speaker
pub fn codec(bus: &'static I2cBus) -> Result<Codec, Error> {
    let mut codec = Codec {
        device: CriticalSectionDevice::new(bus),
        microphone_gain: 0,
    };
    codec.write(&[
        [0x00, 0x80],                // reset: power on, state machine enabled
        [ES8311_CLOCKS, CLOCKS_DAC], // clock manager: MCLK from BCLK, DAC clocks on
        [0x02, 0x18],                // clock manager: MCLK = BCLK × 8 (256 fs at 32 bits a frame)
        [0x0D, 0x01],                // system: analog circuits on
        [ES8311_DAC_POWER, 0x00],    // system: DAC on
        [ES8311_OUTPUT, 0x10],       // system: output to the amplifier on
        [ES8311_DAC_VOLUME, 0xBF],   // DAC volume: 0 dB
        [0x37, 0x08],                // DAC equalizer bypassed
    ])?;
    Ok(codec)
}

impl fmt::Debug for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Codec")
            .field("microphone_gain", &Decibels(self.microphone_gain * 3))
            .finish_non_exhaustive()
    }
}
