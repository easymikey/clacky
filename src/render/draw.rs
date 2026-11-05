//! What the form puts on the screen, as text with terminal escapes.

use unicode_segmentation::UnicodeSegmentation as _;

use crate::render::{
    color::{escape_len, measure_text_width},
    style::Skin,
    symbols::{Guide, Symbols},
    wrap::wrap,
};

const CLEAR: &str = "\x1b[2K";
const CLEAR_BELOW: &str = "\x1b[J";
/// Hides the cursor.
pub const HIDE_CURSOR: &str = "\x1b[?25l";
/// Shows the cursor again.
pub const SHOW_CURSOR: &str = "\x1b[?25h";
const GUTTER: usize = 3;

/// What the answer looks like on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Echo {
    /// The answer is written out as typed.
    Shown,
    /// Every character shows as the mask.
    Masked(Mask),
}

/// The glyph a secret shows as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    /// The `mask` of the session's [`Symbols`].
    Themed,
    /// This character, whatever the symbols.
    Char(char),
}

impl Mask {
    /// The glyph for one character of the secret.
    #[must_use]
    pub fn glyph(self, symbols: &Symbols) -> String {
        match self {
            Self::Themed => symbols.mask.to_owned(),
            Self::Char(mask) => mask.to_string(),
        }
    }
}

/// Wipes the current line and returns to its start.
#[must_use]
pub fn clear_line() -> String {
    format!("\r{CLEAR}")
}

/// The opening line of the rail, with `title`.
#[must_use]
pub fn intro(skin: &Skin, title: &str) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let title = skin.paint(theme.title, &format!(" {} ", title.trim()));
    match skin.guide() {
        Guide::Drawn => format!(
            "{}  {title}\n{}\n",
            skin.paint(theme.rail, symbols.bar_start),
            skin.paint(theme.rail, symbols.bar)
        ),
        Guide::Hidden => format!("{title}\n\n"),
    }
}

/// Draws `rows` over the `previous` rows above; each row ends its line.
#[must_use]
pub fn repaint(previous: usize, rows: &[String]) -> String {
    let up = match previous {
        0 => String::new(),
        lines => format!("\x1b[{lines}A"),
    };
    format!("\r{up}{CLEAR_BELOW}{}", lines_of(rows))
}

pub(crate) fn lines_of(rows: &[String]) -> String {
    rows.iter().flat_map(|row| [row.as_str(), "\n"]).collect()
}

/// `text` as `echo` shows it.
#[must_use]
pub fn masked(echo: Echo, symbols: &Symbols, text: &str) -> String {
    match echo {
        Echo::Masked(mask) => mask.glyph(symbols).repeat(text.graphemes(true).count()),
        Echo::Shown => text.to_owned(),
    }
}

/// A question that has been answered: the label, then `value` on the rail.
#[must_use]
pub fn answered(skin: &Skin, label: &str, value: &str) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let value = rail_rows(value, |line| {
        format!(
            "{}  {}\n",
            skin.paint(theme.rail, symbols.bar),
            skin.paint(theme.hint, line)
        )
    });
    format!(
        "{}  {label}\n{value}{}\n",
        skin.paint(theme.submitted, symbols.step_submit),
        skin.paint(theme.rail, symbols.bar)
    )
}

fn rail_rows(value: &str, row: impl Fn(&str) -> String) -> String {
    match value {
        "" => String::new(),
        value => value.split('\n').map(row).collect(),
    }
}

/// The dim rows of keys under the options of a choice prompt: one row when
/// `keys` fit the `width`, else wrapped under the rail.
#[must_use]
pub fn instructions(skin: &Skin, keys: &str, width: Option<usize>) -> Vec<String> {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let bar = skin.paint(theme.active, symbols.bar);
    let room = width.map(|width| {
        width
            .saturating_sub(measure_text_width(symbols.bar) + 2)
            .max(1)
    });
    let lines = room.map_or_else(|| vec![keys.to_owned()], |room| wrap(keys, room, ""));
    lines
        .iter()
        .map(|line| format!("{bar}  {}", skin.paint(theme.hint, line)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// How a line inside a box is painted.
pub enum NoteTone {
    /// As it is.
    Plain,
    /// Dimmed.
    Faint,
    /// As a warning.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One line inside a box.
pub struct NoteLine {
    /// How the line is painted.
    pub tone: NoteTone,
    /// The text of the line.
    pub text: String,
}

/// Which symbol and color a log entry gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A plain message.
    Message,
    /// Information.
    Info,
    /// A success.
    Success,
    /// A step of a longer process.
    Step,
    /// A warning.
    Warn,
    /// An error.
    Error,
}

impl Tone {
    pub(crate) fn symbol(self, skin: &Skin) -> String {
        let (theme, symbols) = (skin.theme(), skin.symbols());
        match self {
            Self::Message => skin.paint(theme.rail, symbols.bar),
            Self::Info => skin.paint(theme.info, symbols.info),
            Self::Success => skin.paint(theme.success, symbols.success),
            Self::Step => skin.paint(theme.step, symbols.step_submit),
            Self::Warn => skin.paint(theme.warning, symbols.warn),
            Self::Error => skin.paint(theme.error, symbols.error),
        }
    }
}

/// A log entry: the symbol and the first line, the rest on the rail, then a bar
/// so the next element starts below it.
#[must_use]
pub fn log(skin: &Skin, tone: Tone, message: &str) -> String {
    log_entry(skin, &tone.symbol(skin), message, 1)
}

#[must_use]
pub(crate) fn log_entry(
    skin: &Skin,
    symbol: &str,
    message: &str,
    spacing: usize,
) -> String {
    let bar = skin.paint(skin.theme().rail, skin.symbols().bar);
    let rows: String = message
        .split('\n')
        .enumerate()
        .map(|(at, line)| {
            let lead = if at == 0 { symbol } else { &bar };
            match line {
                "" => format!("{lead}\n"),
                line => format!("{lead}  {line}\n"),
            }
        })
        .collect();
    format!("{rows}{}", format!("{bar}\n").repeat(spacing))
}

/// A box with `title` and `lines`, as wide as `width` allows.
#[must_use]
pub fn boxed(
    skin: &Skin,
    title: &str,
    lines: &[NoteLine],
    width: Option<usize>,
) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let room = width.map(|width| width.saturating_sub(GUTTER + 3));
    let shown: Vec<(NoteTone, String)> = lines
        .iter()
        .map(|line| {
            let text = warned(symbols.warn, line.tone, &line.text);
            (line.tone, fitted(&text, room, symbols.ellipsis))
        })
        .collect();
    let painted: Vec<String> = shown
        .iter()
        .map(|(tone, text)| toned(skin, *tone, text))
        .collect();
    let title = fitted(title, room, symbols.ellipsis);
    let inner = painted
        .iter()
        .map(|text| measure_text_width(text))
        .chain([measure_text_width(&title)])
        .max()
        .unwrap_or_default()
        + 2;
    let side = skin.paint(theme.rail, skin.drawn().bar);
    let bar = skin.paint(theme.rail, symbols.bar);
    let blank = format!("{side}{}{side}\n", " ".repeat(inner + 2));
    let body = painted
        .iter()
        .map(|text| {
            let pad = " ".repeat(inner.saturating_sub(measure_text_width(text)));
            format!("{side}  {text}{pad}{side}\n")
        })
        .collect::<Vec<_>>()
        .concat();
    let rule = symbols
        .bar_h
        .repeat(inner.saturating_sub(measure_text_width(&title) + 1).max(1));
    format!(
        "{}  {title} {}\n{blank}{body}{blank}{}\n{bar}\n",
        skin.paint(theme.submitted, symbols.step_submit),
        skin.paint(theme.rail, &format!("{rule}{}", symbols.corner_top_right)),
        skin.paint(
            theme.rail,
            &format!(
                "{}{}{}",
                skin.drawn().connect_left,
                symbols.bar_h.repeat(inner + 2),
                symbols.corner_bottom_right
            )
        ),
    )
}

fn fitted(text: &str, room: Option<usize>, ellipsis: &str) -> String {
    room.map_or_else(|| text.to_owned(), |room| clip_row(text, room, ellipsis))
}

fn warned(warn: &str, tone: NoteTone, text: &str) -> String {
    match tone {
        NoteTone::Warn => format!("{warn} {text}"),
        NoteTone::Plain | NoteTone::Faint => text.to_owned(),
    }
}

fn toned(skin: &Skin, tone: NoteTone, text: &str) -> String {
    let theme = skin.theme();
    match tone {
        NoteTone::Plain => skin.code(text),
        NoteTone::Faint => skin.code_on(text, |part| skin.paint(theme.hint, part)),
        NoteTone::Warn => skin.code_on(text, |part| skin.paint(theme.warning, part)),
    }
}

/// `row` as it is when it fits `width` columns, else cut there with `ellipsis`
/// at the cut; the escapes after the cut stay, so a color is closed.
pub(crate) fn clip_row(row: &str, width: usize, ellipsis: &str) -> String {
    if measure_text_width(row) <= width {
        return row.to_owned();
    }
    let budget = width.saturating_sub(measure_text_width(ellipsis));
    let mut out = String::with_capacity(row.len());
    let (mut used, mut cut, mut linked) = (0, false, false);
    let mut rest = row;
    while !rest.is_empty() {
        if rest.starts_with('\x1b') {
            let (escape, tail) = rest.split_at(escape_len(rest));
            if let Some(link) = escape.strip_prefix("\x1b]8;") {
                linked = link.split_once(';').is_some_and(|(_, uri)| {
                    !uri.trim_end_matches(['\x07', '\x1b', '\\']).is_empty()
                });
            }
            out.push_str(escape);
            rest = tail;
            continue;
        }
        let (text, tail) = rest.split_at(rest.find('\x1b').unwrap_or(rest.len()));
        for grapheme in text.graphemes(true) {
            if cut {
                break;
            }
            let columns = measure_text_width(grapheme);
            if used + columns > budget {
                cut = true;
                out.push_str(ellipsis);
            } else {
                out.push_str(grapheme);
                used += columns;
            }
        }
        rest = tail;
    }
    if linked {
        out.push_str("\x1b]8;;\x1b\\");
    }
    out
}

pub(crate) fn screen_rows(rows: &[String], width: Option<usize>) -> usize {
    width.map_or(rows.len(), |width| {
        rows.iter().map(|row| wrapped_rows(row, width.max(1))).sum()
    })
}

/// The screen rows one `row` takes: the terminal moves a wide grapheme that
/// does not fit the rest of a row to the next one.
fn wrapped_rows(row: &str, width: usize) -> usize {
    let (mut rows, mut used) = (1, 0);
    let mut rest = row;
    while !rest.is_empty() {
        if rest.starts_with('\x1b') {
            rest = rest.split_at(escape_len(rest)).1;
            continue;
        }
        let (text, tail) = rest.split_at(rest.find('\x1b').unwrap_or(rest.len()));
        for grapheme in text.graphemes(true) {
            let columns = measure_text_width(grapheme);
            if used > 0 && used + columns > width {
                rows += 1;
                used = 0;
            }
            used += columns;
        }
        rest = tail;
    }
    rows
}

/// A question backed out of: the label, then `shown` struck through.
#[must_use]
pub fn cancelled(skin: &Skin, label: &str, shown: &str) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let answer = rail_rows(shown, |line| {
        format!(
            "{}  {}\n",
            skin.paint(theme.rail, symbols.bar),
            skin.paint(theme.struck, line)
        )
    });
    format!(
        "{}  {label}\n{answer}{}\n",
        skin.paint(theme.cancelled, symbols.step_cancel),
        skin.paint(theme.rail, symbols.bar)
    )
}

/// The closing line of the rail, with `message`.
#[must_use]
pub fn outro(skin: &Skin, message: &str) -> String {
    match skin.guide() {
        Guide::Drawn => format!(
            "{}  {}\n\n",
            skin.paint(skin.theme().rail, skin.symbols().bar_end),
            skin.code(message)
        ),
        Guide::Hidden => format!("{}\n\n", skin.code(message)),
    }
}

/// The closing line of a form that failed: `error`, then one row for each
/// cause.
#[must_use]
pub fn error_outro(skin: &Skin, error: &str, causes: &[String]) -> String {
    let theme = skin.theme();
    let causes: String = causes
        .iter()
        .map(|detail| format!("   {detail}\n"))
        .collect::<Vec<_>>()
        .concat();
    let error = skin.code_on(error, |part| skin.paint(theme.error, part));
    match skin.guide() {
        Guide::Drawn => format!(
            "{}  {error}\n{causes}\n",
            skin.paint(theme.rail, skin.symbols().bar_end)
        ),
        Guide::Hidden => format!("{error}\n{causes}\n"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::render::{
        color::{ColorChoice, measure_text_width},
        draw::{
            Echo,
            Mask,
            NoteLine,
            NoteTone,
            Tone,
            answered,
            boxed,
            cancelled,
            clip_row,
            error_outro,
            instructions,
            log,
            masked,
            screen_rows,
        },
        style::Skin,
        symbols::Symbols,
    };

    const PLAIN: Skin = Skin::new(ColorChoice::Plain);
    const COLORED: Skin = Skin::new(ColorChoice::Colored);

    #[rstest]
    #[case::fits("abc", 5, "abc")]
    #[case::cut("abcdefg", 5, "abcd…")]
    #[case::wide("バーバー", 5, "バー…")]
    #[case::color_closes("\x1b[2mabcdefg\x1b[0m", 5, "\x1b[2mabcd…\x1b[0m")]
    fn a_row_is_cut_to_the_width_keeping_its_escapes(
        #[case] row: &str,
        #[case] width: usize,
        #[case] clipped: &str,
    ) {
        assert_eq!(clip_row(row, width, "…"), clipped);
        assert!(measure_text_width(&clip_row(row, width, "…")) <= width);
    }

    #[test]
    fn a_link_is_cut_by_its_text_and_closed() {
        let link = "\x1b]8;;https://example.com/docs\x1b\\documentation\x1b]8;;\x1b\\";
        assert_eq!(measure_text_width(link), 13);
        assert_eq!(
            clip_row(link, 6, "…"),
            "\x1b]8;;https://example.com/docs\x1b\\docum…\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn a_link_cut_open_is_closed() {
        let open = "\x1b]8;;https://example.com/docs\x1b\\documentation";
        assert_eq!(
            clip_row(open, 6, "…"),
            "\x1b]8;;https://example.com/docs\x1b\\docum…\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn rows_wider_than_the_screen_count_as_their_wrapped_rows() {
        let rows = ["abc".to_owned(), String::new(), "abcdefghij".to_owned()];
        assert_eq!(screen_rows(&rows, None), 3);
        assert_eq!(screen_rows(&rows, Some(10)), 3);
        assert_eq!(screen_rows(&rows, Some(4)), 1 + 1 + 3);
        assert_eq!(screen_rows(&rows, Some(0)), 3 + 1 + 10);
    }

    #[rstest]
    #[case::fit_evenly("ああああ", 4, 2)]
    #[case::the_odd_column_is_left_empty("あああああ", 5, 3)]
    #[case::three_columns("あああ", 3, 3)]
    #[case::one_fits("あ", 1, 1)]
    #[case::escapes_are_free("\x1b[2mああ\x1b[0mあああ", 5, 3)]
    fn a_wide_grapheme_that_does_not_fit_starts_the_next_screen_row(
        #[case] row: &str,
        #[case] width: usize,
        #[case] rows: usize,
    ) {
        assert_eq!(screen_rows(&[row.to_owned()], Some(width)), rows);
    }

    #[test]
    fn keys_wrap_under_the_rail_on_a_narrow_terminal() {
        let keys = "↑/↓ to navigate • Space: select • Enter: confirm";
        assert_eq!(instructions(&PLAIN, keys, None), [format!("│  {keys}")]);
        assert_eq!(instructions(&PLAIN, keys, Some(80)), [format!("│  {keys}")]);
        let rows = instructions(&PLAIN, keys, Some(24));
        assert!(rows.len() > 1, "{rows:?}");
        assert!(rows.iter().all(|row| row.starts_with("│  ")), "{rows:?}");
        assert!(
            rows.iter().all(|row| measure_text_width(row) <= 24),
            "{rows:?}"
        );
        let words: Vec<&str> = rows.iter().flat_map(|row| row.split(' ')).collect();
        assert!(!words.contains(&"…") && !rows.concat().contains('…'));
        let kept: String = rows
            .iter()
            .filter_map(|row| row.strip_prefix("│  "))
            .collect();
        assert_eq!(kept.replace(' ', ""), keys.replace(' ', ""));
    }

    #[test]
    fn answered_secret_is_a_mask_of_the_same_length() {
        let unicode = Symbols::unicode();
        assert_eq!(
            masked(Echo::Masked(Mask::Themed), &unicode, "123456"),
            "▪▪▪▪▪▪"
        );
        assert_eq!(
            masked(Echo::Masked(Mask::Themed), &Symbols::ascii(), "123456"),
            "******"
        );
        assert_eq!(
            masked(Echo::Masked(Mask::Char('*')), &unicode, "pa ss"),
            "*****"
        );
        assert_eq!(
            masked(Echo::Shown, &unicode, "you@example.com"),
            "you@example.com"
        );
    }

    fn note_line(tone: NoteTone, text: &str) -> NoteLine {
        NoteLine {
            tone,
            text: text.to_owned(),
        }
    }

    fn note() -> Vec<NoteLine> {
        vec![
            note_line(NoteTone::Plain, "Mikhail Avdeev"),
            note_line(NoteTone::Faint, "saved to Keychain · 2 cookies"),
            note_line(NoteTone::Warn, "could not terminate the previous session"),
        ]
    }

    #[test]
    fn boxed_note_is_a_clack_box() {
        let expected = "\
◇  Logged in ──────────────────────────────────╮
│                                              │
│  Mikhail Avdeev                              │
│  saved to Keychain · 2 cookies               │
│  ▲ could not terminate the previous session  │
│                                              │
├──────────────────────────────────────────────╯
│
";
        assert_eq!(boxed(&PLAIN, "Logged in", &note(), Some(120)), expected);
    }

    #[test]
    fn coloured_box_keeps_its_width_and_paints_by_tone() {
        let colour = boxed(&COLORED, "Logged in", &note(), Some(120));
        let widths: Vec<usize> =
            colour.lines().take(7).map(measure_text_width).collect();
        assert!(
            widths.iter().all(|width| Some(width) == widths.first()),
            "{widths:?}"
        );
        assert!(
            colour.contains("\x1b[33m▲ could not terminate"),
            "{colour:?}"
        );
        assert!(colour.contains("\x1b[2msaved to Keychain"), "{colour:?}");
        assert!(!colour.contains("\x1b[1m"), "no bold: {colour:?}");
    }

    #[test]
    fn narrow_terminal_clips_box_lines() {
        let narrow = boxed(&PLAIN, "Logged in", &note(), Some(30));
        assert!(
            narrow.lines().all(|line| measure_text_width(line) <= 30),
            "{narrow}"
        );
        assert!(narrow.contains('…'), "{narrow}");
    }

    #[test]
    fn a_long_title_is_clipped_to_the_terminal() {
        let title = "T".repeat(100);
        let narrow = boxed(&PLAIN, &title, &note(), Some(80));
        assert!(
            narrow.lines().all(|line| measure_text_width(line) <= 80),
            "{narrow}"
        );
        assert!(narrow.lines().next().is_some_and(|top| top.contains('…')));
    }

    #[test]
    fn a_colored_row_is_cut_by_its_columns_and_keeps_whole_escapes() {
        let row = format!("\x1b[1m\x1b[32mCompiling\x1b[0m {}", "x".repeat(90));
        let clipped = clip_row(&row, 20, "…");
        assert_eq!(measure_text_width(&clipped), 20);
        assert!(clipped.starts_with("\x1b[1m\x1b[32mCompiling\x1b[0m x"));
        assert_eq!(clip_row("short", 20, "…"), "short");
    }

    #[test]
    fn code_in_a_box_is_cyan_without_backticks() {
        let lines = [note_line(NoteTone::Plain, "run `acme logout`")];
        let colour = boxed(&COLORED, "T", &lines, Some(80));
        assert!(colour.contains("\x1b[36macme logout\x1b[0m"), "{colour:?}");
        assert!(boxed(&PLAIN, "T", &lines, Some(80)).contains("`acme logout`"));
    }

    #[rstest]
    #[case::message(Tone::Message, "│  hello\n│\n")]
    #[case::info(Tone::Info, "●  hello\n│\n")]
    #[case::success(Tone::Success, "◆  hello\n│\n")]
    #[case::step(Tone::Step, "◇  hello\n│\n")]
    #[case::warn(Tone::Warn, "▲  hello\n│\n")]
    #[case::error(Tone::Error, "■  hello\n│\n")]
    fn plain_log_is_a_symbol_the_message_and_a_bar(
        #[case] tone: Tone,
        #[case] expected: &str,
    ) {
        assert_eq!(log(&PLAIN, tone, "hello"), expected);
    }

    #[rstest]
    #[case::message(Tone::Message, "\x1b[2m│\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    #[case::info(Tone::Info, "\x1b[34m●\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    #[case::success(Tone::Success, "\x1b[32m◆\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    #[case::step(Tone::Step, "\x1b[32m◇\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    #[case::warn(Tone::Warn, "\x1b[33m▲\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    #[case::error(Tone::Error, "\x1b[31m■\x1b[0m  hello\n\x1b[2m│\x1b[0m\n")]
    fn coloured_log_paints_the_symbol_and_keeps_the_bar_grey(
        #[case] tone: Tone,
        #[case] expected: &str,
    ) {
        assert_eq!(log(&COLORED, tone, "hello"), expected);
    }

    #[test]
    fn log_continues_lines_on_the_rail() {
        assert_eq!(
            log(&PLAIN, Tone::Warn, "first\n\nthird"),
            "▲  first\n│\n│  third\n│\n"
        );
        assert_eq!(log(&PLAIN, Tone::Info, ""), "●\n│\n");
    }

    #[test]
    fn cancelled_question_stays_struck_through() {
        assert_eq!(cancelled(&PLAIN, "Code", "▪▪"), "■  Code\n│  ▪▪\n│\n");
        assert_eq!(cancelled(&PLAIN, "Email", ""), "■  Email\n│\n");
        let colour = cancelled(&COLORED, "Email", "you@");
        assert!(colour.contains("\x1b[31m■"), "{colour:?}");
        assert!(colour.contains("\x1b[2m\x1b[9myou@"), "{colour:?}");
        assert!(
            !colour.contains("\x1b[31m│"),
            "the bar stays grey: {colour:?}"
        );
        assert!(colour.contains("\x1b[2m│"), "{colour:?}");
    }

    #[test]
    fn failure_ends_grey_with_a_red_message() {
        let details = ["cause: x".to_owned()];
        assert_eq!(
            error_outro(&PLAIN, "login cancelled", &details),
            "└  login cancelled\n   cause: x\n\n"
        );
        let colour = error_outro(&COLORED, "login cancelled", &[]);
        assert!(colour.starts_with("\x1b[2m└"), "{colour:?}");
        assert!(colour.contains("\x1b[31mlogin cancelled"), "{colour:?}");
    }

    #[test]
    fn no_colour_means_no_escape_codes_but_cursor_moves() {
        let text = answered(&PLAIN, "Email", "you@example.com");
        assert_eq!(text, "◇  Email\n│  you@example.com\n│\n");
    }

    #[rstest]
    #[case::first_draw(0, &["a"], "\r\x1b[Ja\n")]
    #[case::same_height(2, &["a", "b"], "\r\x1b[2A\x1b[Ja\nb\n")]
    #[case::shrinks(3, &["a"], "\r\x1b[3A\x1b[Ja\n")]
    #[case::grows(1, &["a", "b", "c"], "\r\x1b[1A\x1b[Ja\nb\nc\n")]
    fn repaint_goes_up_the_previous_rows_and_clears_below(
        #[case] previous: usize,
        #[case] rows: &[&str],
        #[case] drawn: &str,
    ) {
        let rows: Vec<String> = rows.iter().map(|row| (*row).to_owned()).collect();
        assert_eq!(super::repaint(previous, &rows), drawn);
    }

    #[test]
    fn cursor_escapes_hide_and_show() {
        assert_eq!(super::HIDE_CURSOR, "\x1b[?25l");
        assert_eq!(super::SHOW_CURSOR, "\x1b[?25h");
    }
}
