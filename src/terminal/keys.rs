//! Decodes the bytes a unix terminal in raw mode sends into keys.

use std::{io, time::Duration};

use crate::{
    terminal::char_key,
    values::line::{Csi, Key, csi_final, csi_parameter, ss3_key},
};

const ESC: u8 = 0x1b;

/// How long a lone Esc waits for the rest of a sequence before it is the key.
const LONE_ESCAPE_WAIT: Duration = Duration::from_millis(50);

const SEQUENCE_WAIT: Duration = Duration::from_millis(250);

const LONGEST_KEY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    Done(Key),
    More,
}

/// The next key from `next_byte`, which waits at most the given time and gives
/// `Ok(None)` when it runs out. With no time (before a key's first byte) it
/// waits as long as the question lives, and `Ok(None)` then means the question
/// ended and there is no key.
pub(super) fn read(
    mut next_byte: impl FnMut(Option<Duration>) -> io::Result<Option<u8>>,
) -> io::Result<Option<Key>> {
    let mut bytes = Vec::new();
    loop {
        let wait = match bytes.as_slice() {
            [] => None,
            [ESC] => Some(LONE_ESCAPE_WAIT),
            _ => Some(SEQUENCE_WAIT),
        };
        let Some(byte) = next_byte(wait)? else {
            return Ok((!bytes.is_empty()).then(|| after_silence(&bytes)));
        };
        bytes.push(byte);
        match decode(&bytes) {
            Progress::Done(keystroke) => return Ok(Some(keystroke)),
            Progress::More if bytes.len() >= LONGEST_KEY => {
                return Ok(Some(Key::Ignored));
            }
            Progress::More => {}
        }
    }
}

/// Esc alone is Esc; any other cut-off sequence is lost.
fn after_silence(bytes: &[u8]) -> Key {
    match bytes {
        [ESC] => Key::Cancel,
        _ => Key::Ignored,
    }
}

fn decode(bytes: &[u8]) -> Progress {
    match bytes {
        [] | [ESC] | [ESC, b'O'] => Progress::More,
        [ESC, b'[', rest @ ..] => csi(rest),
        [ESC, b'O', last, ..] => Progress::Done(ss3_key(char::from(*last))),
        [ESC, b'b', ..] => Progress::Done(Key::WordLeft),
        [ESC, b'f', ..] => Progress::Done(Key::WordRight),
        // Alt with another key: wait for all of its bytes, then drop them.
        [ESC, lead, ..] if bytes.len() <= char_len(*lead) => Progress::More,
        [ESC, ..] => Progress::Done(Key::Ignored),
        [lead, ..] if bytes.len() < char_len(*lead) => Progress::More,
        [lead, ..] => Progress::Done(
            std::str::from_utf8(bytes.get(..char_len(*lead)).unwrap_or_default())
                .ok()
                .and_then(|text| text.chars().next())
                .map_or(Key::Ignored, char_key),
        ),
    }
}

const fn char_len(lead: u8) -> usize {
    match lead {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

fn csi(rest: &[u8]) -> Progress {
    let end = rest
        .iter()
        .position(|byte| !csi_parameter(char::from(*byte)));
    match end.and_then(|end| rest.split_at_checked(end)) {
        Some((parameters, [last, ..])) if csi_final(char::from(*last)) => {
            Progress::Done(
                parameters
                    .iter()
                    .fold(Csi::default(), |csi, byte| csi.feed(char::from(*byte)))
                    .key(char::from(*last)),
            )
        }
        Some(_) => Progress::Done(Key::Ignored),
        None => Progress::More,
    }
}

#[cfg(test)]
mod tests {
    use std::{io, time::Duration};

    use rstest::rstest;

    use crate::{
        terminal::keys::{
            LONE_ESCAPE_WAIT,
            Progress,
            SEQUENCE_WAIT,
            after_silence,
            decode,
            read,
        },
        values::line::Key,
    };

    #[rstest]
    #[case::letter(b"a", Key::Char('a'))]
    #[case::cyrillic("ж".as_bytes(), Key::Char('ж'))]
    #[case::emoji("🦀".as_bytes(), Key::Char('🦀'))]
    #[case::enter_cr(b"\r", Key::Enter)]
    #[case::enter_lf(b"\n", Key::Enter)]
    #[case::tab(b"\t", Key::Tab)]
    #[case::back_tab(b"\x1b[Z", Key::Ignored)]
    #[case::backspace_del(b"\x7f", Key::Backspace)]
    #[case::backspace_ctrl_h(b"\x08", Key::Backspace)]
    #[case::ctrl_a(b"\x01", Key::Home)]
    #[case::ctrl_b(b"\x02", Key::Left)]
    #[case::ctrl_c(b"\x03", Key::Interrupt)]
    #[case::ctrl_d(b"\x04", Key::EndOfInput)]
    #[case::ctrl_e(b"\x05", Key::End)]
    #[case::ctrl_f(b"\x06", Key::Right)]
    #[case::ctrl_k(b"\x0b", Key::ClearAfter)]
    #[case::ctrl_u(b"\x15", Key::ClearBefore)]
    #[case::ctrl_w(b"\x17", Key::DeleteWord)]
    #[case::up(b"\x1b[A", Key::Up)]
    #[case::down(b"\x1b[B", Key::Down)]
    #[case::right(b"\x1b[C", Key::Right)]
    #[case::left(b"\x1b[D", Key::Left)]
    #[case::application_up(b"\x1bOA", Key::Up)]
    #[case::application_left(b"\x1bOD", Key::Left)]
    #[case::home(b"\x1b[H", Key::Home)]
    #[case::home_tilde(b"\x1b[1~", Key::Home)]
    #[case::home_rxvt(b"\x1b[7~", Key::Home)]
    #[case::application_home(b"\x1bOH", Key::Home)]
    #[case::end(b"\x1b[F", Key::End)]
    #[case::end_tilde(b"\x1b[4~", Key::End)]
    #[case::end_rxvt(b"\x1b[8~", Key::End)]
    #[case::application_end(b"\x1bOF", Key::End)]
    #[case::delete(b"\x1b[3~", Key::Delete)]
    #[case::insert(b"\x1b[2~", Key::Ignored)]
    #[case::page_up(b"\x1b[5~", Key::Ignored)]
    #[case::page_down(b"\x1b[6~", Key::Ignored)]
    #[case::ctrl_left(b"\x1b[1;5D", Key::WordLeft)]
    #[case::ctrl_right(b"\x1b[1;5C", Key::WordRight)]
    #[case::alt_b(b"\x1bb", Key::WordLeft)]
    #[case::alt_f(b"\x1bf", Key::WordRight)]
    #[case::alt_x(b"\x1bx", Key::Ignored)]
    #[case::alt_cyrillic("\u{1b}ж".as_bytes(), Key::Ignored)]
    #[case::double_escape(b"\x1b\x1b", Key::Ignored)]
    #[case::f5(b"\x1b[15~", Key::Ignored)]
    #[case::broken_utf8(b"\x80", Key::Ignored)]
    #[case::broken_csi(b"\x1b[\x01", Key::Ignored)]
    fn bytes_make_a_key(#[case] bytes: &[u8], #[case] key: Key) {
        assert_eq!(decode(bytes), Progress::Done(key));
    }

    #[rstest]
    #[case::escape(b"\x1b")]
    #[case::csi(b"\x1b[")]
    #[case::csi_parameters(b"\x1b[1;")]
    #[case::ss3(b"\x1bO")]
    #[case::alt_cyrillic("\u{1b}ж".as_bytes().get(..2).unwrap_or_default())]
    #[case::cyrillic("ж".as_bytes().get(..1).unwrap_or_default())]
    #[case::emoji("🦀".as_bytes().get(..3).unwrap_or_default())]
    fn a_started_key_asks_for_more(#[case] bytes: &[u8]) {
        assert_eq!(decode(bytes), Progress::More);
    }

    #[rstest]
    #[case::lone_escape(b"\x1b", Key::Cancel)]
    #[case::cut_sequence(b"\x1b[1;", Key::Ignored)]
    fn silence_ends_a_started_key(#[case] bytes: &[u8], #[case] key: Key) {
        assert_eq!(after_silence(bytes), key);
    }

    fn read_chunks(
        chunks: &[Option<&[u8]>],
    ) -> (io::Result<Option<Key>>, Vec<Option<Duration>>) {
        let mut bytes = chunks
            .iter()
            .flat_map(|chunk| {
                chunk.map_or_else(
                    || vec![None],
                    |bytes| bytes.iter().map(|byte| Some(*byte)).collect(),
                )
            })
            .collect::<std::collections::VecDeque<_>>();
        let mut waits = Vec::new();
        let key = read(|wait| {
            waits.push(wait);
            bytes
                .pop_front()
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
        });
        (key, waits)
    }

    #[test]
    fn a_sequence_split_across_reads_is_one_key() {
        let (key, waits) = read_chunks(&[Some(b"\x1b["), Some(b"1;5"), Some(b"D")]);
        assert_eq!(key.ok(), Some(Some(Key::WordLeft)));
        assert_eq!(
            waits,
            [
                None,
                Some(LONE_ESCAPE_WAIT),
                Some(SEQUENCE_WAIT),
                Some(SEQUENCE_WAIT),
                Some(SEQUENCE_WAIT),
                Some(SEQUENCE_WAIT),
            ]
        );
    }

    #[test]
    fn a_lone_escape_is_cancel_once_nothing_follows() {
        let (key, _) = read_chunks(&[Some(b"\x1b"), None]);
        assert_eq!(key.ok(), Some(Some(Key::Cancel)));
    }

    #[test]
    fn a_cut_sequence_is_dropped_whole() {
        let (key, _) = read_chunks(&[Some(b"\x1b[1"), None]);
        assert_eq!(key.ok(), Some(Some(Key::Ignored)));
    }

    #[test]
    fn a_read_takes_one_key_and_leaves_the_rest() {
        let (key, waits) = read_chunks(&[Some(b"ab")]);
        assert_eq!(key.ok(), Some(Some(Key::Char('a'))));
        assert_eq!(waits, [None]);
    }

    #[test]
    fn no_first_byte_because_the_question_ended_is_no_key() {
        let (key, waits) = read_chunks(&[None]);
        assert_eq!(key.ok(), Some(None));
        assert_eq!(waits, [None]);
    }

    #[test]
    fn a_failed_read_is_the_error() {
        let (key, _) = read_chunks(&[]);
        assert_eq!(
            key.map_err(|error| error.kind()),
            Err(io::ErrorKind::UnexpectedEof)
        );
    }

    #[test]
    fn a_runaway_sequence_is_given_up() {
        let mut long = vec![0x1b, b'['];
        long.extend([b'1'; 40]);
        let (key, _) = read_chunks(&[Some(&long)]);
        assert_eq!(key.ok(), Some(Some(Key::Ignored)));
    }
}
