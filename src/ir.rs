//! The IR transmitter: an IR LED driven by an RMT channel, sending NEC
//! codes, or any remote's as raw pulses.

use esp_hal::{
    Async,
    gpio::Level,
    peripherals::GPIO44,
    rmt::{PulseCode, TxChannelConfig, TxChannelCreator},
};

use crate::{
    Error,
    pulses::{self, MAX_RAW_PULSES, NEC_REPEAT, NEC_STOP},
    remote::RmtTx,
};

/// The IR channel's tick, 1 µs: the RMT's clock divided by this.
const IR_DIVIDER: u8 = 10;

/// The 38 kHz carrier, a third of each cycle on, in ticks of the RMT's clock
/// (the carrier runs on it undivided): 8.8 µs on, 17.5 µs off.
const IR_CARRIER_HIGH: u16 = 88;
const IR_CARRIER_LOW: u16 = 175;

/// The RMT code for a `(mark, space)` pair: the carrier on for `mark` µs,
/// then off for `space`. A zero space ends the code.
const fn burst((mark, space): (u16, u16)) -> PulseCode {
    PulseCode::new(Level::High, mark, Level::Low, space)
}

/// The IR transmitter: an IR LED on GPIO44, driven by an RMT channel with a
/// 38 kHz carrier.
///
/// It sends NEC remote-control codes, the protocol most remotes use, and
/// any other protocol's as raw pulses ([`send_raw`](Self::send_raw)).
#[derive(Debug)]
pub struct IrTransmitter {
    channel: RmtTx,
}

impl IrTransmitter {
    /// The most `(mark, space)` pairs [`send_raw`](Self::send_raw) takes.
    pub const MAX_PULSES: usize = MAX_RAW_PULSES;

    /// The longest mark or space [`send_raw`](Self::send_raw) takes, in µs.
    pub const MAX_DURATION: u16 = pulses::MAX_DURATION;

    /// Sends an NEC code: `address` and `command`, each followed by its
    /// inverse. Returns once it's sent (68 ms).
    ///
    /// # Errors
    ///
    /// If the RMT fails to send it.
    pub async fn send_nec(&mut self, address: u8, command: u8) -> Result<(), Error> {
        self.send_nec_extended(u16::from_le_bytes([address, !address]), command)
            .await
    }

    /// Sends an extended NEC code, whose 16-bit `address` takes the place of
    /// the 8-bit address and its inverse, as many devices use.
    ///
    /// # Errors
    ///
    /// If the RMT fails to send it.
    pub async fn send_nec_extended(&mut self, address: u16, command: u8) -> Result<(), Error> {
        let mut codes = [PulseCode::end_marker(); 34];
        for (code, pulse) in codes.iter_mut().zip(pulses::nec(address, command)) {
            *code = burst(pulse);
        }
        self.transmit(&codes).await
    }

    /// Sends NEC's repeat code: the button of the last code is still held.
    /// Remotes send it every 108 ms, counted from the start of the code.
    ///
    /// # Errors
    ///
    /// If the RMT fails to send it.
    pub async fn send_nec_repeat(&mut self) -> Result<(), Error> {
        self.transmit(&[burst(NEC_REPEAT), burst(NEC_STOP)]).await
    }

    /// Sends a code given as its pulses, for protocols other than NEC (Sony,
    /// RC5, Samsung, an air conditioner's) or timings recorded from a
    /// remote: each `(mark, space)` is the carrier on for `mark` µs, then
    /// off for `space` µs. Returns once it's sent.
    ///
    /// Codes longer than the RMT channel's memory (48 pulses) are fed to it
    /// from its interrupt as they go out.
    ///
    /// # Errors
    ///
    /// [`Error::Ir`] if there are no pulses or more than
    /// [`MAX_PULSES`](Self::MAX_PULSES), a mark or space is zero (only the
    /// last space may be) or longer than [`MAX_DURATION`](Self::MAX_DURATION),
    /// or the RMT fails to send it.
    pub async fn send_raw(&mut self, pulses: &[(u16, u16)]) -> Result<(), Error> {
        if !pulses::is_sendable(pulses) {
            return Err(Error::Ir);
        }
        // The pulses, then the end marker.
        let mut codes = [PulseCode::end_marker(); MAX_RAW_PULSES + 1];
        for (code, &pulse) in codes.iter_mut().zip(pulses) {
            *code = burst(pulse);
        }
        self.transmit(&codes[..=pulses.len()]).await
    }

    async fn transmit(&mut self, codes: &[PulseCode]) -> Result<(), Error> {
        self.channel.transmit(codes).await.map_err(|_| Error::Ir)
    }
}

/// Sets up the IR transmitter on an RMT transmit channel of [`rmt`]'s.
///
/// # Errors
///
/// If the channel can't be configured.
///
/// [`rmt`]: crate::rmt
pub fn ir_transmitter(
    channel: impl TxChannelCreator<'static, Async>,
    pin: GPIO44<'static>,
) -> Result<IrTransmitter, Error> {
    let config = TxChannelConfig::default()
        .with_clk_divider(IR_DIVIDER)
        .with_idle_output(true)
        .with_idle_output_level(Level::Low)
        .with_carrier_modulation(true)
        .with_carrier_high(IR_CARRIER_HIGH)
        .with_carrier_low(IR_CARRIER_LOW)
        .with_carrier_level(Level::High);
    let channel = channel
        .configure_tx(&config)
        .map_err(|_| Error::Rmt)?
        .with_pin(pin);
    Ok(IrTransmitter { channel })
}
