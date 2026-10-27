//! The IR transmitter: an IR LED driven by an RMT channel, sending NEC
//! codes.

use esp_hal::{
    Async,
    gpio::Level,
    peripherals::GPIO44,
    rmt::{PulseCode, TxChannelConfig, TxChannelCreator},
};

use crate::{Error, remote::RmtTx};

/// The IR channel's tick, 1 µs: the RMT's clock divided by this.
const IR_DIVIDER: u8 = 10;

/// The 38 kHz carrier, a third of each cycle on, in ticks of the RMT's clock
/// (the carrier runs on it undivided): 8.8 µs on, 17.5 µs off.
const IR_CARRIER_HIGH: u16 = 88;
const IR_CARRIER_LOW: u16 = 175;

/// NEC's pulses, in 1 µs ticks: a burst of the carrier, then a space.
const NEC_START: PulseCode = PulseCode::new(Level::High, 9000, Level::Low, 4500);
const NEC_REPEAT: PulseCode = PulseCode::new(Level::High, 9000, Level::Low, 2250);
const NEC_ZERO: PulseCode = PulseCode::new(Level::High, 562, Level::Low, 563);
const NEC_ONE: PulseCode = PulseCode::new(Level::High, 562, Level::Low, 1688);
/// The burst that closes the last space; its zero-length space is the end
/// marker.
const NEC_STOP: PulseCode = PulseCode::new(Level::High, 562, Level::Low, 0);

/// The IR transmitter: an IR LED on GPIO44, driven by an RMT channel with a
/// 38 kHz carrier. It sends NEC remote-control codes, the protocol most
/// remotes use.
#[derive(Debug)]
pub struct IrTransmitter {
    channel: RmtTx,
}

impl IrTransmitter {
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
        let [low, high] = address.to_le_bytes();
        let data = u32::from_le_bytes([low, high, command, !command]);
        let mut codes = [NEC_STOP; 34];
        codes[0] = NEC_START;
        // Least significant bit first.
        for (bit, code) in (0..32).zip(&mut codes[1..]) {
            *code = if (data >> bit) & 1 == 1 {
                NEC_ONE
            } else {
                NEC_ZERO
            };
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
        self.transmit(&[NEC_REPEAT, NEC_STOP]).await
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
