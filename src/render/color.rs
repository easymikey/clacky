//! Whether output carries color, and how wide text is on screen.

use unicode_width::UnicodeWidthStr as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Whether output carries color escapes.
pub enum ColorChoice {
    /// No escapes.
    Plain,
    /// Colors on.
    Colored,
}

impl ColorChoice {
    /// `Colored` when `on`, else `Plain`.
    #[must_use]
    pub const fn colored_if(on: bool) -> Self {
        if on {
            ColorChoice::Colored
        } else {
            ColorChoice::Plain
        }
    }
}

/// The columns `text` takes on screen; escape sequences take none.
#[must_use]
pub fn measure_text_width(text: &str) -> usize {
    let (mut columns, mut rest) = (0, text);
    while let Some(at) = rest.find('\x1b') {
        let (plain, escaped) = rest.split_at(at);
        columns += plain.width();
        rest = escaped.split_at(escape_len(escaped)).1;
    }
    columns + rest.width()
}

/// The bytes of the escape sequence at the start of `text`, which starts with
/// ESC: a CSI up to its final byte (0x40 to 0x7e), a `]` sequence up to
/// `BEL` or ESC `\`, else the character after ESC.
pub(crate) fn escape_len(text: &str) -> usize {
    let mut chars = text.char_indices().skip(1);
    match chars.next() {
        Some((_, '[')) => chars
            .find(|&(_, c)| ('\u{40}'..='\u{7e}').contains(&c))
            .map_or(text.len(), |(at, c)| at + c.len_utf8()),
        Some((_, ']')) => [
            text.find('\x07').map(|at| at + 1),
            text.find("\x1b\\").map(|at| at + 2),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(text.len()),
        Some((at, c)) => at + c.len_utf8(),
        None => text.len(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::render::color::measure_text_width;

    #[rstest]
    #[case::plain("abc", 3)]
    #[case::wide("バー", 4)]
    #[case::styled("\x1b[36m\x1b[1mabc\x1b[0m", 3)]
    #[case::cursor_move("\x1b[2Kab\x1b[1A", 2)]
    #[case::bare_escape("a\x1bxb", 2)]
    #[case::link_st("\x1b]8;;https://example.com\x1b\\docs\x1b]8;;\x1b\\", 4)]
    #[case::link_bel("\x1b]8;;https://example.com\x07docs\x1b]8;;\x07", 4)]
    fn width_skips_escapes(#[case] text: &str, #[case] columns: usize) {
        assert_eq!(measure_text_width(text), columns);
    }
}
