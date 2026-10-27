//! The microphone: an I2S stream from the ES8311 codec's ADC.

use core::{cell::Cell, convert::Infallible, fmt};

use esp_hal::{
    Async,
    dma::DmaRxStreamBuf,
    dma_rx_stream_buffer,
    i2s::master::{I2sRx, I2sRxDmaTransfer},
};

use crate::Error;

/// The DMA buffer for the microphone: 4096 bytes is 1024 stereo frames,
/// 23 ms, in 8 chunks. Samples wait at most that long to be taken.
const MICROPHONE_BUFFER: usize = 4096;
const MICROPHONE_CHUNK: usize = 512;

/// Frames dropped after the stream (re)starts: the input joins the codec's
/// stream mid-frame, so the first one is misaligned.
const SETTLING_FRAMES: usize = 8;

type MicrophoneTransfer = I2sRxDmaTransfer<'static, Async, DmaRxStreamBuf>;

/// The microphone: an I2S stream from the ES8311 codec's ADC, on the
/// speaker's clocks, which [`Audio`] runs.
///
/// [`Audio`]: crate::Audio
pub struct Microphone {
    /// `None` only while restarting.
    transfer: Option<MicrophoneTransfer>,
    /// Frames still to drop since the stream (re)started.
    settling: usize,
}

impl Microphone {
    /// Starts the stream on `rx`.
    ///
    /// # Errors
    ///
    /// If the DMA transfer can't be started.
    ///
    /// # Panics
    ///
    /// If called twice (its DMA buffer is static).
    pub fn start(rx: I2sRx<'static, Async>) -> Result<Self, Error> {
        let buffer = dma_rx_stream_buffer!(MICROPHONE_BUFFER, MICROPHONE_CHUNK);
        let transfer = rx.read(buffer).map_err(|_| Error::Microphone)?;
        Ok(Self {
            transfer: Some(transfer),
            settling: SETTLING_FRAMES,
        })
    }

    /// Hands the mono samples heard to `take`, up to 256 at a time,
    /// forever. When `resettle` is set (the clocks stopped and started), the
    /// first frames after are dropped, as after a restart.
    ///
    /// # Errors
    ///
    /// Returns only if the stream couldn't be restarted.
    pub async fn run(
        &mut self,
        resettle: &Cell<bool>,
        mut take: impl FnMut(&[i16]),
    ) -> Result<Infallible, Error> {
        let mut mono = [0i16; 256];
        loop {
            let transfer = self.transfer.as_mut().ok_or(Error::Microphone)?;
            if transfer.wait_for_available_async().await.is_err() {
                self.restart()?;
                continue;
            }
            if resettle.take() {
                self.settling = SETTLING_FRAMES;
            }
            loop {
                // Stereo frames: the codec's one ADC is in both channels.
                let (frames, _) = transfer.peek().as_chunks::<4>();
                if frames.is_empty() {
                    break;
                }
                let dropped = frames.len().min(self.settling);
                if dropped > 0 {
                    self.settling -= dropped;
                    transfer.consume(dropped * 4);
                    continue;
                }
                let count = frames.len().min(mono.len());
                for (sample, &[lo, hi, _, _]) in mono.iter_mut().zip(frames) {
                    *sample = i16::from_le_bytes([lo, hi]);
                }
                transfer.consume(count * 4);
                take(&mono[..count]);
            }
        }
    }

    /// Starts the stream again after it overflowed, on the same memory.
    fn restart(&mut self) -> Result<(), Error> {
        let transfer = self.transfer.take().ok_or(Error::Microphone)?;
        let (rx, buffer) = transfer.stop();
        let (descriptors, bytes) = buffer.split();
        let buffer = DmaRxStreamBuf::new(descriptors, bytes).map_err(|_| Error::Microphone)?;
        self.transfer = Some(rx.read(buffer).map_err(|_| Error::Microphone)?);
        self.settling = SETTLING_FRAMES;
        Ok(())
    }
}

impl fmt::Debug for Microphone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Microphone")
            .field("running", &self.transfer.is_some())
            .finish_non_exhaustive()
    }
}
