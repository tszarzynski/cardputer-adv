//! The pulse trains the RMT sends, worked out apart from it: a WS2812
//! colour's bits, and remote-control codes as `(mark, space)` pairs, the
//! carrier on for `mark` µs and then off for `space` µs.

use core::iter;

/// The longest mark or space the RMT can time: 15 bits of 1 µs ticks.
pub const MAX_DURATION: u16 = 0x7FFF;

/// The most pulses a raw IR code can have (enough for an air-conditioner
/// remote's).
pub const MAX_RAW_PULSES: usize = 128;

/// NEC's start: a 9 ms burst, then 4.5 ms off.
pub const NEC_START: (u16, u16) = (9000, 4500);
/// NEC's repeat code's start, sent alone while a button is held.
pub const NEC_REPEAT: (u16, u16) = (9000, 2250);
const NEC_ZERO: (u16, u16) = (562, 563);
const NEC_ONE: (u16, u16) = (562, 1688);
/// The burst that closes the last space; nothing follows it.
pub const NEC_STOP: (u16, u16) = (562, 0);

/// A WS2812 colour's 24 bits as it takes them: green, red, then blue, each
/// most significant bit first.
pub fn ws2812_bits(red: u8, green: u8, blue: u8) -> impl Iterator<Item = bool> {
    let grb = u32::from_be_bytes([0, green, red, blue]);
    (0..24).rev().map(move |bit| (grb >> bit) & 1 == 1)
}

/// An extended NEC code's pulses: the start, the 16-bit `address`, then
/// `command` and its inverse, each least significant bit first, and the
/// stop burst. 34 pairs.
pub fn nec(address: u16, command: u8) -> impl Iterator<Item = (u16, u16)> {
    let [low, high] = address.to_le_bytes();
    let data = u32::from_le_bytes([low, high, command, !command]);
    let bits = (0..32).map(move |bit| {
        if (data >> bit) & 1 == 1 {
            NEC_ONE
        } else {
            NEC_ZERO
        }
    });
    iter::once(NEC_START)
        .chain(bits)
        .chain(iter::once(NEC_STOP))
}

/// Whether `pulses` can be sent as a raw code: at most [`MAX_RAW_PULSES`],
/// each mark and space at most [`MAX_DURATION`], and none of them zero
/// (the RMT takes a zero as the end), but for the last space.
pub fn is_sendable(pulses: &[(u16, u16)]) -> bool {
    let in_range = |duration: u16| (1..=MAX_DURATION).contains(&duration);
    let Some((&(last_mark, last_space), rest)) = pulses.split_last() else {
        return false;
    };
    pulses.len() <= MAX_RAW_PULSES
        && rest
            .iter()
            .all(|&(mark, space)| in_range(mark) && in_range(space))
        && in_range(last_mark)
        && last_space <= MAX_DURATION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws2812_takes_green_red_blue_most_significant_bit_first() {
        // Green's top bit first, red's lowest bit last of its byte.
        let set: Vec<usize> = ws2812_bits(0x01, 0x80, 0x00)
            .enumerate()
            .filter_map(|(i, bit)| bit.then_some(i))
            .collect();
        assert_eq!(set, [0, 15]);
        assert_eq!(ws2812_bits(0xFF, 0xFF, 0xFF).count(), 24);
    }

    #[test]
    fn nec_sends_the_address_and_command_and_their_inverses() {
        let pulses: Vec<_> = nec(u16::from_le_bytes([0x00, !0x00]), 0x45).collect();
        assert_eq!(pulses.len(), 34);
        assert_eq!(pulses[0], NEC_START);
        assert_eq!(pulses[33], NEC_STOP);
        let bits: Vec<bool> = pulses[1..33]
            .iter()
            .map(|&pulse| pulse == NEC_ONE)
            .collect();
        let byte = |i: usize| {
            bits[i * 8..(i + 1) * 8]
                .iter()
                .rev()
                .fold(0u8, |byte, &bit| (byte << 1) | u8::from(bit))
        };
        assert_eq!(
            [byte(0), byte(1), byte(2), byte(3)],
            [0x00, 0xFF, 0x45, 0xBA]
        );
        // Every code has 16 ones and 16 zeros: 68 ms in all.
        let total: u32 = pulses
            .iter()
            .map(|&(mark, space)| u32::from(mark) + u32::from(space))
            .sum();
        assert_eq!(total, 68_062);
    }

    #[test]
    fn raw_codes_must_fit_the_rmt() {
        assert!(is_sendable(&[(9000, 4500), (562, 0)]));
        assert!(is_sendable(&[(MAX_DURATION, MAX_DURATION)]));
        assert!(!is_sendable(&[]));
        // A zero ends the code early, unless it's the last space.
        assert!(!is_sendable(&[(9000, 0), (562, 0)]));
        assert!(!is_sendable(&[(0, 4500)]));
        assert!(!is_sendable(&[(MAX_DURATION + 1, 100)]));
        assert!(is_sendable(&[(500, 500); MAX_RAW_PULSES]));
        assert!(!is_sendable(&[(500, 500); MAX_RAW_PULSES + 1]));
    }
}
