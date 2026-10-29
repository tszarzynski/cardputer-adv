//! The keyboard's layout against cardputer-adv-keyboard's, where it came
//! from: every key, with and without Shift and Fn, types the same.

use std::{cell::RefCell, collections::VecDeque, convert::Infallible, rc::Rc};

use cardputer_adv_host_tests::Keymap;
use cardputer_adv_keyboard::Keyboard;
use embedded_hal::i2c::{ErrorType, I2c, Operation};

/// A TCA8418 whose FIFO is shared with the test, to fill.
struct FakeTca8418 {
    fifo: Rc<RefCell<VecDeque<u8>>>,
    /// The register a read reads.
    register: u8,
}

impl ErrorType for FakeTca8418 {
    type Error = Infallible;
}

impl I2c for FakeTca8418 {
    fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Infallible> {
        for operation in operations {
            match operation {
                // A register to read; longer writes set one (the
                // configuration, or flags cleared).
                Operation::Write(bytes) => {
                    if let [register] = bytes {
                        self.register = *register;
                    }
                }
                Operation::Read(buffer) => {
                    let mut fifo = self.fifo.borrow_mut();
                    buffer[0] = match self.register {
                        // KEY_LCK_EC: how many events there are.
                        0x03 => u8::try_from(fifo.len().min(10)).unwrap_or(10),
                        // KEY_EVENT_A: the oldest, or 0 once it's empty.
                        0x04 => fifo.pop_front().unwrap_or(0),
                        _ => 0,
                    };
                }
            }
        }
        Ok(())
    }
}

const fn press(number: u8) -> u8 {
    number | 0x80
}

/// What `events` (up to the FIFO's 10) type, by cardputer-adv-keyboard
/// and by [`Keymap`]: as `Debug` text, as their types are named alike.
fn typed(events: &[u8]) -> (Vec<String>, Vec<String>) {
    let fifo = Rc::new(RefCell::new(VecDeque::new()));
    let mut keyboard = Keyboard::new(FakeTca8418 {
        fifo: Rc::clone(&fifo),
        register: 0,
    })
    .expect("the fake answers");
    fifo.borrow_mut().extend(events);
    let theirs = keyboard
        .inputs()
        .expect("the fake answers")
        .map(|input| format!("{input:?}"))
        .collect();
    let mut keymap = Keymap::default();
    let ours = events
        .iter()
        .filter_map(|&event| keymap.input(event))
        .map(|input| format!("{input:?}"))
        .collect();
    (theirs, ours)
}

/// The key numbers the TCA8418 reports for the matrix's rows 0–6 and
/// columns 0–7: its row × 10 + column + 1.
fn key_numbers() -> impl Iterator<Item = u8> {
    (0..7).flat_map(|row| (0..8).map(move |column| row * 10 + column + 1))
}

/// The key number that types `input` on its own, by cardputer-adv-keyboard.
fn key_typing(input: &str) -> u8 {
    key_numbers()
        .find(|&number| typed(&[press(number)]).0 == [input])
        .expect("a key types it")
}

#[test]
fn every_key_types_the_same_with_and_without_shift_and_fn() {
    let shift = key_typing("Modifier(Shift)");
    let fn_key = key_typing("Modifier(Fn)");
    for held in [&[][..], &[shift], &[fn_key], &[shift, fn_key]] {
        for number in key_numbers() {
            let events: Vec<u8> = held
                .iter()
                .map(|&modifier| press(modifier))
                .chain([press(number), number])
                .chain(held.iter().copied())
                .collect();
            let (theirs, ours) = typed(&events);
            assert!(!theirs.is_empty(), "key {number} types nothing");
            assert_eq!(ours, theirs, "key {number} with {held:?} held");
        }
    }
}
