//! Rows of typed text with a caret drawn into them, for prompts whose rows
//! are painted by the machine (the real cursor stays hidden there).

use unicode_segmentation::UnicodeSegmentation as _;

use crate::render::{
    color::{ColorChoice, measure_text_width},
    style::Skin,
};

type Cell = (String, usize);

fn cell(text: String) -> Cell {
    let columns = measure_text_width(&text);
    (text, columns)
}

/// Rows of `text` with the caret at byte `at`, each logical line wrapped to
/// `room` columns.
pub(crate) fn rows(
    skin: &Skin,
    (text, at): (&str, usize),
    placeholder: &str,
    room: Option<usize>,
) -> Vec<String> {
    if text.is_empty() {
        return placeholder
            .split('\n')
            .zip(0..)
            .flat_map(|(line, number)| {
                wrapped(placeholder_cells(skin, line, number == 0), room)
            })
            .collect();
    }
    let mut start = 0_usize;
    text.split('\n')
        .flat_map(|line| {
            let end = start.saturating_add(line.len());
            let caret = (start..=end).contains(&at).then(|| {
                line.get(..at.saturating_sub(start))
                    .map_or(0, |before| before.graphemes(true).count())
            });
            start = end.saturating_add(1);
            wrapped(text_cells(skin, line, caret), room)
        })
        .collect()
}

/// Where the colour cannot mark the caret, a thin bar goes before the
/// character.
fn caret_cell(skin: &Skin, under: &str) -> Cell {
    match skin.color() {
        ColorChoice::Colored => cell(skin.paint(
            skin.theme().cursor,
            if under.is_empty() { " " } else { under },
        )),
        ColorChoice::Plain => cell(format!("{}{under}", skin.symbols().caret)),
    }
}

/// The caret sits before grapheme `caret`, and after the last one when it is
/// their count.
fn text_cells(skin: &Skin, line: &str, caret: Option<usize>) -> Vec<Cell> {
    let mut cells: Vec<Cell> = line
        .graphemes(true)
        .zip(0..)
        .map(|(grapheme, place)| {
            if caret == Some(place) {
                caret_cell(skin, grapheme)
            } else {
                cell(grapheme.to_owned())
            }
        })
        .collect();
    if caret == Some(cells.len()) {
        cells.push(caret_cell(skin, ""));
    }
    cells
}

fn placeholder_cells(skin: &Skin, line: &str, first: bool) -> Vec<Cell> {
    let mut graphemes = line.graphemes(true);
    let head = if first {
        Some(caret_cell(skin, graphemes.next().unwrap_or("")))
    } else {
        None
    };
    head.into_iter()
        .chain(
            graphemes
                .map(|grapheme| cell(skin.paint(skin.theme().placeholder, grapheme))),
        )
        .collect()
}

/// A row has at least one cell, however wide.
fn wrapped(cells: Vec<Cell>, room: Option<usize>) -> Vec<String> {
    let room = room.unwrap_or(usize::MAX);
    let (mut rows, row, _) = cells.into_iter().fold(
        (Vec::new(), String::new(), 0_usize),
        |(mut rows, row, used), (shown, columns)| {
            if used > 0 && used.saturating_add(columns) > room {
                rows.push(row);
                (rows, shown, columns)
            } else {
                (rows, row + &shown, used.saturating_add(columns))
            }
        },
    );
    rows.push(row);
    rows
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::rows;
    use crate::render::{color::ColorChoice, style::Skin};

    const PLAIN: Skin = Skin::new(ColorChoice::Plain);
    const COLORED: Skin = Skin::new(ColorChoice::Colored);

    #[rstest]
    #[case::end("ab", 2, None, &["ab▏"])]
    #[case::middle("ab", 1, None, &["a▏b"])]
    #[case::start("ab", 0, None, &["▏ab"])]
    #[case::second_line("ab\ncd", 4, None, &["ab", "c▏d"])]
    #[case::end_of_first_line("ab\ncd", 2, None, &["ab▏", "cd"])]
    #[case::empty_line_with_caret("ab\n", 3, None, &["ab", "▏"])]
    #[case::wrapped("abcd", 4, Some(2), &["ab", "cd", "▏"])]
    #[case::the_mark_takes_a_column("abcd", 0, Some(2), &["▏a", "bc", "d"])]
    #[case::wide_characters("日本語", 6, Some(4), &["日本", "▏語"])]
    fn the_caret_stays_in_its_row(
        #[case] text: &str,
        #[case] at: usize,
        #[case] room: Option<usize>,
        #[case] expected: &[&str],
    ) {
        assert_eq!(rows(&PLAIN, (text, at), "", room), expected);
    }

    #[rstest]
    #[case::nothing("", &["▏"])]
    #[case::hint("type", &["▏type"])]
    #[case::two_lines("one\ntwo", &["▏one", "two"])]
    fn an_empty_text_shows_the_placeholder(
        #[case] placeholder: &str,
        #[case] expected: &[&str],
    ) {
        assert_eq!(rows(&PLAIN, ("", 0), placeholder, None), expected);
    }

    #[test]
    fn colour_inverts_the_character_under_the_caret() {
        assert_eq!(rows(&COLORED, ("ab", 1), "", None), ["a\x1b[7mb\x1b[0m"]);
        assert_eq!(rows(&COLORED, ("ab", 2), "", None), ["ab\x1b[7m \x1b[0m"]);
    }

    #[test]
    fn coloured_placeholder_inverts_its_first_character_and_dims_the_rest() {
        assert_eq!(
            rows(&COLORED, ("", 0), "hi", None),
            ["\x1b[7mh\x1b[0m\x1b[2mi\x1b[0m"]
        );
    }
}
