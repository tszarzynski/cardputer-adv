//! The speaker and the microphone together: I2S streams to and from the
//! ES8311 codec, on the same clocks.

use core::{cell::Cell, convert::Infallible, fmt};

use embassy_futures::select::{Either, select};
use esp_hal::{i2s::master::I2s, peripherals::GPIO46};

use crate::{
    Error, SpeakerPins,
    microphone::Microphone,
    speaker::{Speaker, i2s_config},
};

/// The peripherals the speaker and the microphone use together: the
/// speaker's, and the codec's ADC output.
#[derive(Debug)]
pub struct AudioPins {
    /// The speaker's peripherals, whose clocks the microphone runs on too.
    pub speaker: SpeakerPins,
    /// The codec's ADC output (ASDOUT).
    pub din: GPIO46<'static>,
}

/// The speaker and the microphone, started by [`audio`]. The microphone
/// runs on the speaker stream's clocks (as the codec does), so the two run
/// together, in [`run`](Self::run).
///
/// The microphone hears nothing until [`Codec::set_microphone`] turns it on.
///
/// [`Codec::set_microphone`]: crate::Codec::set_microphone
pub struct Audio {
    speaker: Speaker,
    microphone: Microphone,
}

impl Audio {
    /// Plays the mono samples `fill` writes, and hands the mono samples
    /// heard to `take`, both at [`SAMPLE_RATE`], forever. To only listen,
    /// fill with silence: `|samples| samples.fill(0)`.
    ///
    /// `fill` is asked for samples, and `take` given up to 256, every few
    /// milliseconds. Each stream buffers 23 ms; one that falls behind by more
    /// restarts, so its sound has a gap (the speaker's restarts on silence).
    /// Don't block in either (a log line over USB can take milliseconds),
    /// and to keep up while other tasks hold the CPU, run this on a
    /// higher-priority executor, as for [`Speaker::run`].
    ///
    /// # Errors
    ///
    /// Returns only if a stream couldn't be restarted.
    ///
    /// [`SAMPLE_RATE`]: crate::SAMPLE_RATE
    pub async fn run(
        &mut self,
        fill: impl FnMut(&mut [i16]),
        take: impl FnMut(&[i16]),
    ) -> Result<Infallible, Error> {
        // When the speaker's stream restarts, its clocks stop and start: the
        // microphone drops what comes first, as when its own does.
        let resettle = Cell::new(false);
        let speaker = self.speaker.stream(fill, || resettle.set(true));
        let microphone = self.microphone.run(&resettle, take);
        match select(speaker, microphone).await {
            Either::First(stopped) | Either::Second(stopped) => stopped,
        }
    }
}

/// Starts the I2S streams to and from the codec, for the speaker and the
/// microphone together.
///
/// The speaker plays silence until [`Audio::run`] feeds it. Then call [`codec`], and turn the microphone on with
/// [`Codec::set_microphone`].
///
/// # Errors
///
/// If the I2S peripheral or its DMA can't be set up.
///
/// # Panics
///
/// If called twice, or with [`speaker`] (the DMA buffers are static).
///
/// [`codec`]: crate::codec
/// [`Codec::set_microphone`]: crate::Codec::set_microphone
/// [`speaker`]: crate::speaker
pub fn audio(pins: AudioPins) -> Result<Audio, Error> {
    let SpeakerPins {
        i2s,
        dma,
        bclk,
        ws,
        dout,
    } = pins.speaker;
    // The input runs on the output's bit and word clocks, which the codec
    // runs on too.
    let i2s = I2s::new(i2s, dma, i2s_config().with_signal_loopback(true))
        .map_err(|_| Error::Speaker)?
        .into_async();
    let rx = i2s.i2s_rx.with_din(pins.din).build();
    let tx = i2s
        .i2s_tx
        .with_bclk(bclk)
        .with_ws(ws)
        .with_dout(dout)
        .build();
    Ok(Audio {
        speaker: Speaker::start(tx)?,
        microphone: Microphone::start(rx)?,
    })
}

impl fmt::Debug for Audio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Audio")
            .field("speaker", &self.speaker)
            .field("microphone", &self.microphone)
            .finish()
    }
}
