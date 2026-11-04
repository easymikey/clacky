//! Maps Windows console key records to keys. It calls no Windows API, so the
//! table is tested on every OS.

use std::io;

use crate::{terminal::char_key, values::line::Key};

const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0d;
const VK_ESCAPE: u16 = 0x1b;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_DELETE: u16 = 0x2e;

const RIGHT_ALT_PRESSED: u32 = 0x1;
const LEFT_ALT_PRESSED: u32 = 0x2;
const RIGHT_CTRL_PRESSED: u32 = 0x4;
const LEFT_CTRL_PRESSED: u32 = 0x8;
const SHIFT_PRESSED: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyInput {
    pub(super) unit: u16,
    pub(super) code: u16,
    pub(super) state: u32,
}

impl KeyInput {
    const fn ctrl(self) -> bool {
        self.state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0
    }

    /// Alt, but not the Ctrl and Alt that make `AltGr`.
    const fn alt(self) -> bool {
        self.state & (LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) != 0 && !self.ctrl()
    }

    const fn shift(self) -> bool {
        self.state & SHIFT_PRESSED != 0
    }
}

/// The key of a press; `None` for keys that mean nothing here, such as Shift
/// alone, F-keys or a broken UTF-16 pair. `next` is called only when the first
/// half of a surrogate pair needs its second.
pub(super) fn keystroke_of(
    press: KeyInput,
    next: impl FnOnce() -> io::Result<KeyInput>,
) -> io::Result<Option<Key>> {
    Ok(match press.unit {
        0 => virtual_key(press),
        0x09 if press.shift() => Some(Key::Ignored),
        0xd800..=0xdbff => char::decode_utf16([press.unit, next()?.unit])
            .next()
            .and_then(Result::ok)
            .map(char_key),
        unit if press.alt() => Some(match char::from_u32(u32::from(unit)) {
            Some('b') => Key::WordLeft,
            Some('f') => Key::WordRight,
            _ => Key::Ignored,
        }),
        unit => char::decode_utf16([unit])
            .next()
            .and_then(Result::ok)
            .map(char_key),
    })
}

fn virtual_key(press: KeyInput) -> Option<Key> {
    let by_word = press.ctrl();
    match press.code {
        VK_LEFT if by_word => Some(Key::WordLeft),
        VK_RIGHT if by_word => Some(Key::WordRight),
        VK_LEFT => Some(Key::Left),
        VK_RIGHT => Some(Key::Right),
        VK_UP => Some(Key::Up),
        VK_DOWN => Some(Key::Down),
        VK_HOME => Some(Key::Home),
        VK_END => Some(Key::End),
        VK_DELETE => Some(Key::Delete),
        VK_BACK => Some(Key::Backspace),
        VK_RETURN => Some(Key::Enter),
        VK_ESCAPE => Some(Key::Cancel),
        VK_TAB if press.shift() => Some(Key::Ignored),
        VK_TAB => Some(Key::Tab),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use rstest::rstest;

    use crate::{
        terminal::console_keys::{
            KeyInput,
            LEFT_ALT_PRESSED,
            LEFT_CTRL_PRESSED,
            RIGHT_ALT_PRESSED,
            RIGHT_CTRL_PRESSED,
            SHIFT_PRESSED,
            VK_BACK,
            VK_DELETE,
            VK_DOWN,
            VK_END,
            VK_ESCAPE,
            VK_HOME,
            VK_LEFT,
            VK_RETURN,
            VK_RIGHT,
            VK_TAB,
            VK_UP,
            keystroke_of,
        },
        values::line::Key,
    };

    const NONE: u32 = 0;

    const fn press(unit: u16, code: u16, state: u32) -> KeyInput {
        KeyInput { unit, code, state }
    }

    fn unit(c: char) -> u16 {
        c.encode_utf16(&mut [0; 2])
            .first()
            .copied()
            .unwrap_or_default()
    }

    fn lone(press: KeyInput) -> Option<Key> {
        keystroke_of(press, || Err(io::Error::other("no second half was needed")))
            .ok()
            .flatten()
    }

    #[rstest]
    #[case::letter(press(unit('a'), 0x41, NONE), Key::Char('a'))]
    #[case::shifted(press(unit('A'), 0x41, SHIFT_PRESSED), Key::Char('A'))]
    #[case::cyrillic(press(unit('ж'), 0x00, NONE), Key::Char('ж'))]
    #[case::alt_gr(
        press(unit('@'), 0x51, LEFT_CTRL_PRESSED | RIGHT_ALT_PRESSED),
        Key::Char('@')
    )]
    #[case::enter(press(0x0d, VK_RETURN, NONE), Key::Enter)]
    #[case::ctrl_enter(press(0x0a, VK_RETURN, LEFT_CTRL_PRESSED), Key::Enter)]
    #[case::enter_without_unit(press(0, VK_RETURN, NONE), Key::Enter)]
    #[case::tab(press(0x09, VK_TAB, NONE), Key::Tab)]
    #[case::tab_without_unit(press(0, VK_TAB, NONE), Key::Tab)]
    #[case::back_tab(press(0x09, VK_TAB, SHIFT_PRESSED), Key::Ignored)]
    #[case::back_tab_without_unit(press(0, VK_TAB, SHIFT_PRESSED), Key::Ignored)]
    #[case::backspace(press(0x08, VK_BACK, NONE), Key::Backspace)]
    #[case::ctrl_backspace(press(0x7f, VK_BACK, LEFT_CTRL_PRESSED), Key::Backspace)]
    #[case::backspace_without_unit(press(0, VK_BACK, NONE), Key::Backspace)]
    #[case::escape(press(0x1b, VK_ESCAPE, NONE), Key::Cancel)]
    #[case::escape_without_unit(press(0, VK_ESCAPE, NONE), Key::Cancel)]
    #[case::ctrl_a(press(0x01, 0x41, LEFT_CTRL_PRESSED), Key::Home)]
    #[case::ctrl_b(press(0x02, 0x42, LEFT_CTRL_PRESSED), Key::Left)]
    #[case::ctrl_c(press(0x03, 0x43, LEFT_CTRL_PRESSED), Key::Interrupt)]
    #[case::ctrl_d(press(0x04, 0x44, LEFT_CTRL_PRESSED), Key::EndOfInput)]
    #[case::ctrl_e(press(0x05, 0x45, RIGHT_CTRL_PRESSED), Key::End)]
    #[case::ctrl_f(press(0x06, 0x46, LEFT_CTRL_PRESSED), Key::Right)]
    #[case::ctrl_k(press(0x0b, 0x4b, LEFT_CTRL_PRESSED), Key::ClearAfter)]
    #[case::ctrl_u(press(0x15, 0x55, LEFT_CTRL_PRESSED), Key::ClearBefore)]
    #[case::ctrl_w(press(0x17, 0x57, LEFT_CTRL_PRESSED), Key::DeleteWord)]
    #[case::up(press(0, VK_UP, NONE), Key::Up)]
    #[case::down(press(0, VK_DOWN, NONE), Key::Down)]
    #[case::left(press(0, VK_LEFT, NONE), Key::Left)]
    #[case::right(press(0, VK_RIGHT, NONE), Key::Right)]
    #[case::home(press(0, VK_HOME, NONE), Key::Home)]
    #[case::end(press(0, VK_END, NONE), Key::End)]
    #[case::delete(press(0, VK_DELETE, NONE), Key::Delete)]
    #[case::ctrl_left(press(0, VK_LEFT, LEFT_CTRL_PRESSED), Key::WordLeft)]
    #[case::ctrl_right(press(0, VK_RIGHT, RIGHT_CTRL_PRESSED), Key::WordRight)]
    #[case::alt_b(press(unit('b'), 0x42, LEFT_ALT_PRESSED), Key::WordLeft)]
    #[case::alt_f(press(unit('f'), 0x46, LEFT_ALT_PRESSED), Key::WordRight)]
    #[case::alt_x(press(unit('x'), 0x58, LEFT_ALT_PRESSED), Key::Ignored)]
    #[case::alt_escape(press(0x1b, VK_ESCAPE, LEFT_ALT_PRESSED), Key::Ignored)]
    fn a_press_makes_a_key(#[case] input: KeyInput, #[case] key: Key) {
        assert_eq!(lone(input), Some(key));
    }

    #[rstest]
    #[case::shift_alone(press(0, 0x10, SHIFT_PRESSED))]
    #[case::control_alone(press(0, 0x11, LEFT_CTRL_PRESSED))]
    #[case::insert(press(0, 0x2d, NONE))]
    #[case::page_up(press(0, 0x21, NONE))]
    #[case::f5(press(0, 0x74, NONE))]
    #[case::lone_low_surrogate(press(0xdc00, 0x00, NONE))]
    fn a_press_without_a_key_is_skipped(#[case] input: KeyInput) {
        assert_eq!(lone(input), None);
    }

    #[test]
    fn a_surrogate_pair_is_one_character() {
        let [high, low] = {
            let mut units = [0; 2];
            '🦀'.encode_utf16(&mut units);
            units
        };
        let key = keystroke_of(press(high, 0x00, NONE), || Ok(press(low, 0x00, NONE)));
        assert_eq!(key.ok(), Some(Some(Key::Char('🦀'))));
    }

    #[test]
    fn a_high_surrogate_without_its_low_half_is_skipped() {
        let key = keystroke_of(press(0xd83e, 0x00, NONE), || {
            Ok(press(unit('a'), 0x41, NONE))
        });
        assert_eq!(key.ok(), Some(None));
    }

    #[test]
    fn a_failed_read_of_the_second_half_is_the_error() {
        let key = keystroke_of(press(0xd83e, 0x00, NONE), || {
            Err(io::Error::from(io::ErrorKind::UnexpectedEof))
        });
        assert_eq!(
            key.err().map(|error| error.kind()),
            Some(io::ErrorKind::UnexpectedEof)
        );
    }
}
