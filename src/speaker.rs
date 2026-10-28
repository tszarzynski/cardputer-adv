//! The speaker: an I2S stream to the ES8311 codec. With the microphone,
//! both streams start together ([`audio`]).
//!
//! [`audio`]: crate::audio

use core::{convert::Infallible, fmt};

use esp_hal::{
    Async,
    dma::DmaTxStreamBuf,
    dma_tx_stream_buffer,
    i2s::master::{Channels, DataFormat, I2s, I2sTx, I2sTxDmaTransfer, TdmConfig},
    peripherals::{DMA_CH0, GPIO41, GPIO42, GPIO43, I2S0},
    time::Rate,
};

use crate::Error;

/// The speaker's sample rate.
pub const SAMPLE_RATE: u32 = 44_100;

/// The DMA buffer: 4096 bytes is 1024 stereo frames, 23 ms, in 8 chunks.
/// Samples are pushed at most that far ahead of being heard.
const SPEAKER_BUFFER: usize = 4096;
const SPEAKER_CHUNK: usize = 512;

/// The peripherals the speaker uses.
#[expect(missing_docs, reason = "the fields are the pins they're named after")]
#[derive(Debug)]
pub struct SpeakerPins {
    pub i2s: I2S0<'static>,
    pub dma: DMA_CH0<'static>,
    pub bclk: GPIO41<'static>,
    pub ws: GPIO43<'static>,
    pub dout: GPIO42<'static>,
}

type SpeakerTransfer = I2sTxDmaTransfer<'static, Async, DmaTxStreamBuf>;

/// Bytes of silence pushed at a time when (re)starting the stream.
const SILENCE: [u8; 64] = [0; 64];

/// The speaker: an I2S stream to the ES8311 codec, playing silence until
/// [`run`](Self::run) feeds it.
pub struct Speaker {
    /// `None` only while restarting.
    transfer: Option<SpeakerTransfer>,
}

impl Speaker {
    /// Plays the mono samples `fill` writes, at [`SAMPLE_RATE`], forever.
    ///
    /// `fill` is asked for as many as the buffer has room for, every few
    /// milliseconds. If it falls behind by more than the buffer (23 ms),
    /// the DMA stops; the stream then restarts on silence, so the sound
    /// skips rather than stops. To keep up while other tasks hold the CPU
    /// for long (drawing a frame, say), run this on a higher-priority
    /// executor, such as esp-rtos's `InterruptExecutor`.
    ///
    /// # Errors
    ///
    /// Returns only if the stream couldn't be restarted.
    pub async fn run(&mut self, fill: impl FnMut(&mut [i16])) -> Result<Infallible, Error> {
        self.stream(fill, || ()).await
    }

    /// [`run`](Self::run), calling `restarted` each time the stream restarts.
    pub(crate) async fn stream(
        &mut self,
        mut fill: impl FnMut(&mut [i16]),
        mut restarted: impl FnMut(),
    ) -> Result<Infallible, Error> {
        let mut mono = [0i16; 256];
        loop {
            let transfer = self.transfer.as_mut().ok_or(Error::Speaker)?;
            if transfer.wait_for_available_async().await.is_err() {
                self.restart()?;
                restarted();
                continue;
            }
            // As much as there's room for.
            while transfer.push_with(|bytes| {
                // Stereo frames, the same sample left and right. The chunks
                // are multiples of 4 bytes, so a frame never straddles two,
                // and while there is room, there is room for a whole frame.
                debug_assert!(bytes.is_empty() || bytes.len() >= 4);
                let frames = (bytes.len() / 4).min(mono.len());
                fill(&mut mono[..frames]);
                for (frame, sample) in bytes.chunks_exact_mut(4).zip(&mono[..frames]) {
                    let [lo, hi] = sample.to_le_bytes();
                    frame.copy_from_slice(&[lo, hi, lo, hi]);
                }
                frames * 4
            }) > 0
            {}
        }
    }

    /// Starts the stream on `tx`, playing silence.
    ///
    /// # Errors
    ///
    /// If the DMA transfer can't be started.
    ///
    /// # Panics
    ///
    /// If called twice (its DMA buffer is static).
    pub(crate) fn start(tx: I2sTx<'static, Async>) -> Result<Self, Error> {
        let mut buffer = dma_tx_stream_buffer!(SPEAKER_BUFFER, SPEAKER_CHUNK);
        // Start full of silence.
        while buffer.push(&SILENCE) > 0 {}
        let transfer = tx.write(buffer).map_err(|_| Error::Speaker)?;
        Ok(Self {
            transfer: Some(transfer),
        })
    }

    /// Starts the stream again after it ran dry: a fresh buffer of silence
    /// on the same memory. The bit clock, and so the codec's clock, stopped
    /// with it; the codec carries on when the clock does.
    fn restart(&mut self) -> Result<(), Error> {
        let transfer = self.transfer.take().ok_or(Error::Speaker)?;
        let (tx, buffer) = transfer.stop();
        let (descriptors, bytes) = buffer.split();
        let mut buffer = DmaTxStreamBuf::new(descriptors, bytes).map_err(|_| Error::Speaker)?;
        while buffer.push(&SILENCE) > 0 {}
        self.transfer = Some(tx.write(buffer).map_err(|_| Error::Speaker)?);
        Ok(())
    }
}

/// Starts the I2S stream to the codec (16-bit stereo at [`SAMPLE_RATE`]),
/// playing silence. The codec takes its clock from it, so call
/// [`codec`] once this is running.
///
/// # Errors
///
/// If the I2S peripheral or its DMA can't be set up.
///
/// # Panics
///
/// If called twice (its DMA buffer is static).
///
/// [`codec`]: crate::codec
pub fn speaker(pins: SpeakerPins) -> Result<Speaker, Error> {
    let i2s = I2s::new(pins.i2s, pins.dma, i2s_config())
        .map_err(|_| Error::Speaker)?
        .into_async();
    let tx = i2s
        .i2s_tx
        .with_bclk(pins.bclk)
        .with_ws(pins.ws)
        .with_dout(pins.dout)
        .build();
    Speaker::start(tx)
}

/// The I2S format to and from the codec: Philips, 16-bit stereo at
/// [`SAMPLE_RATE`].
pub fn i2s_config() -> TdmConfig {
    TdmConfig::new_tdm_philips()
        .with_sample_rate(Rate::from_hz(SAMPLE_RATE))
        .with_data_format(DataFormat::Data16Channel16)
        .with_channels(Channels::STEREO)
}

impl fmt::Debug for Speaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Speaker")
            .field("running", &self.transfer.is_some())
            .finish_non_exhaustive()
    }
}
