//! Word wrap by display columns, aware of SGR escapes.

use unicode_segmentation::UnicodeSegmentation as _;

use crate::render::color::{escape_len, measure_text_width};

const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy)]
enum Item<'a> {
    Escape(&'a str),
    Grapheme(&'a str),
}

impl Item<'_> {
    fn columns(self) -> usize {
        match self {
            Item::Escape(_) => 0,
            Item::Grapheme(grapheme) => measure_text_width(grapheme),
        }
    }
}

/// Rows of at most `width` columns, each led by `prefix` (which counts toward
/// the width). Rows break at spaces; a longer word breaks between graphemes;
/// `\n` breaks stay.
pub(crate) fn wrap(text: &str, width: usize, prefix: &str) -> Vec<String> {
    let mut wrapper = Wrapper {
        avail: width.saturating_sub(measure_text_width(prefix)).max(1),
        prefix,
        rows: Vec::new(),
        row: String::new(),
        used: 0,
        has_word: false,
        active: Vec::new(),
    };
    for line in text.split('\n') {
        wrapper.line(line);
    }
    wrapper.rows
}

struct Wrapper<'p> {
    avail: usize,
    prefix: &'p str,
    rows: Vec<String>,
    row: String,
    used: usize,
    has_word: bool,
    active: Vec<String>,
}

impl Wrapper<'_> {
    fn line(&mut self, line: &str) {
        let mut pending: Vec<Item<'_>> = Vec::new();
        for (is_space, word) in runs(line) {
            if is_space {
                pending = word;
                continue;
            }
            let gap = columns(&pending);
            let fits = self.used + gap + columns(&word) <= self.avail;
            if !self.has_word || fits {
                self.place(&pending);
            } else {
                self.break_row();
                self.place_escapes(&pending);
            }
            self.place(&word);
            self.has_word = true;
            pending.clear();
        }
        if self.used + columns(&pending) <= self.avail {
            self.place(&pending);
        } else {
            self.place_escapes(&pending);
        }
        self.break_row();
    }

    fn place(&mut self, items: &[Item<'_>]) {
        for item in items {
            match *item {
                Item::Escape(sequence) => self.escape(sequence),
                Item::Grapheme(grapheme) => {
                    let width = measure_text_width(grapheme);
                    if self.used > 0 && self.used + width > self.avail {
                        self.break_row();
                    }
                    self.row.push_str(grapheme);
                    self.used += width;
                }
            }
        }
    }

    fn place_escapes(&mut self, items: &[Item<'_>]) {
        for item in items {
            if let Item::Escape(sequence) = *item {
                self.escape(sequence);
            }
        }
    }

    fn escape(&mut self, sequence: &str) {
        self.row.push_str(sequence);
        if let Some(params) = sequence
            .strip_prefix("\x1b[")
            .and_then(|rest| rest.strip_suffix('m'))
        {
            if params.is_empty() || params == "0" {
                self.active.clear();
            } else {
                self.active.push(sequence.to_owned());
            }
        }
    }

    fn break_row(&mut self) {
        if !self.active.is_empty() {
            self.row.push_str(RESET);
        }
        let reopened = self.active.concat();
        let done = std::mem::replace(&mut self.row, reopened);
        self.rows.push(format!("{}{done}", self.prefix));
        self.used = 0;
        self.has_word = false;
    }
}

fn columns(items: &[Item<'_>]) -> usize {
    items.iter().map(|item| item.columns()).sum()
}

fn runs(line: &str) -> Vec<(bool, Vec<Item<'_>>)> {
    let mut runs: Vec<(bool, Vec<Item<'_>>)> = Vec::new();
    let mut carry: Vec<Item<'_>> = Vec::new();
    for item in items(line) {
        match item {
            Item::Escape(_) => match runs.last_mut() {
                Some((_, run)) => run.push(item),
                None => carry.push(item),
            },
            Item::Grapheme(grapheme) => {
                let is_space = grapheme == " ";
                match runs.last_mut() {
                    Some((kind, run)) if *kind == is_space => run.push(item),
                    Some(_) | None => {
                        carry.push(item);
                        runs.push((is_space, std::mem::take(&mut carry)));
                    }
                }
            }
        }
    }
    if runs.is_empty() && !carry.is_empty() {
        runs.push((false, carry));
    }
    runs
}

fn items(line: &str) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find('\x1b') {
        let (plain, escaped) = rest.split_at(at);
        items.extend(plain.graphemes(true).map(Item::Grapheme));
        let len = escape_len(escaped);
        let (sequence, after) = escaped.split_at(len);
        items.push(Item::Escape(sequence));
        rest = after;
    }
    items.extend(rest.graphemes(true).map(Item::Grapheme));
    items
}

#[cfg(test)]
mod tests {
    use proptest::prelude::{ProptestConfig, proptest};
    use rstest::rstest;

    use super::wrap;
    use crate::render::color::measure_text_width;

    #[rstest]
    #[case::words("one two three", 7, "", &["one two", "three"])]
    #[case::hard_break("abcdefg", 3, "", &["abc", "def", "g"])]
    #[case::newlines("a\n\nb", 5, "", &["a", "", "b"])]
    #[case::wide("ab世界", 4, "", &["ab世", "界"])]
    #[case::wide_alone("世", 1, "", &["世"])]
    #[case::empty("", 4, "", &[""])]
    #[case::spaces_kept("a  b", 10, "", &["a  b"])]
    #[case::indent_kept("  a", 10, "", &["  a"])]
    #[case::break_drops_spaces("ab   cd", 4, "", &["ab", "cd"])]
    #[case::long_after_short("ab cdefgh", 4, "", &["ab", "cdef", "gh"])]
    #[case::prefix("one two", 6, "│  ", &["│  one", "│  two"])]
    #[case::style_reopens(
        "\x1b[31mab cd\x1b[0m",
        2,
        "",
        &["\x1b[31mab\x1b[0m", "\x1b[31mcd\x1b[0m"]
    )]
    #[case::escape_has_no_width("\x1b[1mab\x1b[0m cd", 5, "", &["\x1b[1mab\x1b[0m cd"])]
    fn rows_break_by_columns(
        #[case] text: &str,
        #[case] width: usize,
        #[case] prefix: &str,
        #[case] rows: &[&str],
    ) {
        assert_eq!(wrap(text, width, prefix), rows);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, rng_seed: proptest::test_runner::RngSeed::Fixed(0x5eed), ..ProptestConfig::default() })]

        #[test]
        fn no_row_is_wider_than_the_width(
            text in "[a-z世界 \n]{0,60}",
            width in 2_usize..12,
        ) {
            for row in wrap(&text, width, "") {
                assert!(measure_text_width(&row) <= width, "{row:?} in {width}");
            }
        }

        #[test]
        fn text_survives_the_rows(
            text in "[a-z世界 \n]{0,60}",
            width in 2_usize..12,
        ) {
            let kept = |s: &str| s.chars().filter(|c| !matches!(c, ' ' | '\n')).collect::<String>();
            assert_eq!(kept(&wrap(&text, width, "").concat()), kept(&text));
        }
    }
}
