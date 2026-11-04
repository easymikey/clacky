//! Several lines of free text, each on the rail.

use std::marker::PhantomData;

use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt},
        error::Result,
    },
    prompts::{
        caret,
        text::{Plain, WithInitial, WithPlaceholder},
    },
    render::{draw, messages::Messages},
    values::line::{Cancellation, CharFilter, TypedAnswer},
};

/// What ends a [`Multiline`] answer; Enter always adds a line otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubmitKey {
    /// Clack's rule: Enter on a new, empty last line, with the caret at the
    /// end, ends the answer; the empty line is not part of it.
    #[default]
    Enter,
    /// Ctrl-D ends the answer, wherever the caret is; the text is kept as
    /// typed.
    CtrlD,
}

/// Asks for several lines. Enter adds a line; see [`SubmitKey`] for what
/// ends the answer, Esc cancels.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let notes: String = ui.ask(
///     multiline("Release notes?")
///         .initial("- ")
///         .submit(SubmitKey::CtrlD),
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// A placeholder shows only while nothing is typed, so a question cannot have
/// both a placeholder and an initial text:
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = multiline("Notes").initial("- ").placeholder("What changed?");
/// ```
#[must_use]
pub fn multiline(message: impl Into<String>) -> Multiline {
    Multiline {
        message: message.into(),
        placeholder: String::new(),
        initial: String::new(),
        submit: SubmitKey::default(),
        start: PhantomData,
    }
}

/// The question [`multiline`] builds; `S` tells how it starts:
/// [`Plain`], [`WithPlaceholder`] or [`WithInitial`].
#[derive(Debug)]
pub struct Multiline<S = Plain> {
    message: String,
    placeholder: String,
    initial: String,
    submit: SubmitKey,
    start: PhantomData<S>,
}

impl Multiline {
    /// Dim text shown while nothing is typed.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Multiline<WithPlaceholder> {
        Multiline {
            message: self.message,
            placeholder: text.into(),
            initial: self.initial,
            submit: self.submit,
            start: PhantomData,
        }
    }

    /// Text typed already, the caret after it; it can contain newlines. A
    /// terminal that reads whole lines types its line after it.
    #[must_use]
    pub fn initial(self, text: impl Into<String>) -> Multiline<WithInitial> {
        Multiline {
            message: self.message,
            placeholder: self.placeholder,
            initial: text.into(),
            submit: self.submit,
            start: PhantomData,
        }
    }
}

impl<S> Multiline<S> {
    /// What ends the answer; clack's double Enter unless told otherwise.
    #[must_use]
    pub fn submit(self, key: SubmitKey) -> Self {
        Self {
            submit: key,
            ..self
        }
    }

    fn enter(&self, lines: Lines) -> Next<Lines> {
        let text = lines.editor.text();
        let end = text.len();
        let ends = self.submit == SubmitKey::Enter
            && lines.editor.cursor() == end
            && text.ends_with('\n');
        if ends {
            Next::Submit(Lines::at(lines.editor.cut(end.saturating_sub(1), end)))
        } else {
            Next::Editing(Lines::at(lines.editor.inserted('\n')))
        }
    }

    fn end_of_input(&self, lines: Lines) -> Next<Lines> {
        match self.submit {
            SubmitKey::CtrlD => Next::Submit(lines),
            SubmitKey::Enter if lines.editor.text().is_empty() => {
                Next::Quit(lines, Cancellation::Cancel)
            }
            SubmitKey::Enter => Next::Editing(lines),
        }
    }

    fn rows(&self, lines: &Lines, bar: &str, frame: &mut Frame) {
        let skin = frame.skin();
        let room = frame.width().map(|width| width.saturating_sub(4).max(1));
        let keys = match self.submit {
            SubmitKey::Enter => &frame.messages().keys_multiline_enter,
            SubmitKey::CtrlD => &frame.messages().keys_multiline_ctrl_d,
        };
        frame.extend(
            caret::rows(
                skin,
                (lines.editor.text(), lines.editor.cursor()),
                &self.placeholder,
                room,
            )
            .into_iter()
            .map(|row| format!("{bar}  {row}"))
            .chain(draw::instructions(skin, keys, frame.width())),
        );
    }
}

/// The text typed so far, where the caret is, and the column Up and Down
/// aim for once they started moving.
#[derive(Debug)]
pub struct Lines {
    editor: TypedAnswer,
    column: Option<usize>,
}

impl Lines {
    const fn at(editor: TypedAnswer) -> Self {
        Self {
            editor,
            column: None,
        }
    }

    fn moved(self, target: fn(&str, usize) -> usize) -> Self {
        let to = target(self.editor.text(), self.editor.cursor());
        Self::at(self.editor.moved(to))
    }

    fn cut(self, span: fn(&str, usize) -> (usize, usize)) -> Self {
        let (from, to) = span(self.editor.text(), self.editor.cursor());
        Self::at(self.editor.cut(from, to))
    }

    /// Keeps the column the first vertical move started from, so a short line
    /// does not pull the caret left.
    fn vertical(self, target: fn(&str, usize, usize) -> usize) -> Self {
        let (text, at) = (self.editor.text(), self.editor.cursor());
        let want = self.column.unwrap_or_else(|| column(text, at));
        let to = target(text, at, want);
        Self {
            editor: self.editor.moved(to),
            column: Some(want),
        }
    }
}

fn unfinished(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

impl<S> Prompt for Multiline<S> {
    type Output = String;
    type State = Lines;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Lines> {
        let editor = self.initial.split('\n').zip(0..).fold(
            TypedAnswer::new(CharFilter::Printable),
            |editor, (part, at)| {
                let editor = if at > 0 {
                    editor.inserted('\n')
                } else {
                    editor
                };
                editor.prefilled(part)
            },
        );
        Ok(Lines::at(editor))
    }

    fn update(&self, lines: Lines, key: Key) -> Next<Lines> {
        match key {
            Key::Enter => self.enter(lines),
            Key::EndOfInput => self.end_of_input(lines),
            Key::Interrupt => Next::Quit(lines, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(lines, Cancellation::Cancel),
            Key::Up => Next::Editing(lines.vertical(above)),
            Key::Down => Next::Editing(lines.vertical(below)),
            Key::Home => Next::Editing(lines.moved(line_start)),
            Key::End => Next::Editing(lines.moved(line_end)),
            Key::ClearBefore => Next::Editing(lines.cut(joining_before)),
            Key::ClearAfter => Next::Editing(lines.cut(joining_after)),
            Key::Char(_)
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::WordLeft
            | Key::WordRight
            | Key::DeleteWord
            | Key::Tab
            | Key::Ignored => Next::Editing(Lines::at(lines.editor.edit(key))),
        }
    }

    fn view(&self, lines: &Lines, phase: Phase<'_>, frame: &mut Frame) {
        let (theme, symbols) = (frame.theme(), frame.symbols());
        match phase {
            Phase::Active => {
                let bar = frame.paint(theme.active, symbols.bar);
                self.rows(lines, &bar, frame);
            }
            Phase::Warned(_) => {
                let bar = frame.paint(theme.warning, symbols.bar);
                self.rows(lines, &bar, frame);
            }
            Phase::Submitted => frame.row(lines.editor.text()),
            Phase::Cancelled => frame.row(unfinished(lines.editor.text())),
        }
    }

    fn finish(self, lines: Lines) -> Result<String> {
        let mut text = lines.editor.into_text();
        Ok(std::mem::take(&mut *text))
    }

    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        let end: &[Key] = match self.submit {
            SubmitKey::Enter => &[Key::Enter, Key::Enter],
            SubmitKey::CtrlD => &[Key::EndOfInput],
        };
        line.chars()
            .map(Key::Char)
            .chain(end.iter().copied())
            .collect()
    }
}

impl<S> Peek for Multiline<S> {
    type Peeked<'a>
        = &'a str
    where
        Self: 'a;

    fn peek<'a>(&'a self, lines: &'a Lines) -> Option<&'a str> {
        Some(lines.editor.text())
    }
}

impl<S> Prefill for Multiline<S> {
    fn shown_for(&self, answer: &String, _messages: &Messages) -> Option<String> {
        Some(answer.clone())
    }
}

fn line_start(text: &str, at: usize) -> usize {
    text.get(..at)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |newline| newline + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text.get(at..)
        .and_then(|after| after.find('\n'))
        .map_or(text.len(), |newline| at + newline)
}

/// At the line start the newline before it goes, which joins the line to the
/// one above.
fn joining_before(text: &str, at: usize) -> (usize, usize) {
    let start = line_start(text, at);
    (
        if start == at {
            start.saturating_sub(1)
        } else {
            start
        },
        at,
    )
}

/// At the line end the newline after it goes, which joins the line below.
fn joining_after(text: &str, at: usize) -> (usize, usize) {
    let end = line_end(text, at);
    (
        at,
        if end == at && end < text.len() {
            end + 1
        } else {
            end
        },
    )
}

fn at_column(text: &str, (start, end): (usize, usize), column: usize) -> usize {
    text.get(start..end)
        .and_then(|line| line.grapheme_indices(true).nth(column))
        .map_or(end, |(offset, _)| start + offset)
}

fn column(text: &str, at: usize) -> usize {
    text.get(line_start(text, at)..at)
        .map_or(0, |before| before.graphemes(true).count())
}

fn above(text: &str, at: usize, want: usize) -> usize {
    line_start(text, at).checked_sub(1).map_or(at, |end| {
        at_column(text, (line_start(text, end), end), want)
    })
}

fn below(text: &str, at: usize, want: usize) -> usize {
    let end = line_end(text, at);
    if end >= text.len() {
        return at;
    }
    let start = end + 1;
    at_column(text, (start, line_end(text, start)), want)
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use super::{SubmitKey, multiline};
    use crate::{
        core::{
            ask::{Peek, Phase, Prompt},
            error::{Error, Result},
            session::Session,
            settings::Settings,
        },
        fake::{
            Script,
            screen,
            testing::{self, after, bare, keys, rendered as rendered_by},
        },
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    fn live(spec: &str) -> Session<Script> {
        bare(Script::new(TerminalKind::Live, keys(spec), []))
    }

    fn outcome(result: &Result<String>) -> String {
        testing::outcome(result, |text| format!("{text:?}"))
    }

    fn rendered(spec: &str, phase: Phase<'_>, color: ColorChoice) -> Vec<String> {
        rendered_by(&multiline("Notes?"), keys(spec), phase, color, 40)
    }

    #[rstest]
    #[case::second_enter_submits(SubmitKey::Enter, "a\nb\n\n", "\"a\\nb\"")]
    #[case::nothing_typed(SubmitKey::Enter, "\n\n", "\"\"")]
    #[case::ctrl_d_after_text_does_nothing(SubmitKey::Enter, "a\n⌁", "no input")]
    #[case::enter_in_the_middle_adds_a_line(
        SubmitKey::Enter,
        "ab←\n⇲\n\n",
        "\"a\\nb\""
    )]
    #[case::ctrl_d_on_nothing_cancels(SubmitKey::Enter, "⌁", "cancelled")]
    #[case::escape(SubmitKey::Enter, "a␛", "cancelled")]
    #[case::ctrl_c(SubmitKey::Enter, "a☠", "interrupted")]
    #[case::ctrl_d_submits(SubmitKey::CtrlD, "a\nb⌁", "\"a\\nb\"")]
    #[case::ctrl_d_keeps_the_text_as_typed(SubmitKey::CtrlD, "a\n⌁", "\"a\\n\"")]
    #[case::ctrl_d_on_nothing_submits_nothing(SubmitKey::CtrlD, "⌁", "\"\"")]
    #[case::enter_never_submits_here(SubmitKey::CtrlD, "a\n\n⌁", "\"a\\n\\n\"")]
    #[case::up_keeps_the_column(SubmitKey::CtrlD, "ab\ncd↑x⌁", "\"abx\\ncd\"")]
    #[case::up_stops_at_a_short_line(SubmitKey::CtrlD, "a\nbcd↑x⌁", "\"ax\\nbcd\"")]
    #[case::down_keeps_the_column(SubmitKey::CtrlD, "ab\ncd↑⇤→↓x⌁", "\"ab\\ncxd\"")]
    #[case::up_on_the_first_line_stays(SubmitKey::CtrlD, "ab↑x⌁", "\"abx\"")]
    #[case::down_on_the_last_line_stays(SubmitKey::CtrlD, "ab↓x⌁", "\"abx\"")]
    #[case::the_column_survives_a_short_line_up(
        SubmitKey::CtrlD,
        "abcd\nx\nabcd↑↑Z⌁",
        "\"abcdZ\\nx\\nabcd\""
    )]
    #[case::the_column_survives_a_short_line_down(
        SubmitKey::CtrlD,
        "abcd\nx\nabcd↑↑⇤→→→↓↓Z⌁",
        "\"abcd\\nx\\nabcZd\""
    )]
    #[case::another_key_forgets_the_column(
        SubmitKey::CtrlD,
        "abcd\nx\nabcd↑←↑Z⌁",
        "\"Zabcd\\nx\\nabcd\""
    )]
    #[case::backspace_joins_lines(SubmitKey::CtrlD, "a\n⌫b⌁", "\"ab\"")]
    #[case::home_and_end_stay_on_the_line(
        SubmitKey::CtrlD,
        "ab\ncd⇤x↑⇲y⌁",
        "\"aby\\nxcd\""
    )]
    #[case::ctrl_u_clears_to_the_line_start(SubmitKey::CtrlD, "ab\ncd⇞⌁", "\"ab\\n\"")]
    #[case::ctrl_u_at_the_line_start_joins_the_line_above(
        SubmitKey::CtrlD,
        "ab\n⇞x⌁",
        "\"abx\""
    )]
    #[case::ctrl_u_at_the_very_start_does_nothing(
        SubmitKey::CtrlD,
        "ab⇤⇞x⌁",
        "\"xab\""
    )]
    #[case::ctrl_k_clears_to_the_line_end(SubmitKey::CtrlD, "ab\ncd↑←⇟⌁", "\"a\\ncd\"")]
    #[case::ctrl_k_at_the_line_end_joins_the_line_below(
        SubmitKey::CtrlD,
        "ab\ncd↑⇲⇟x⌁",
        "\"abxcd\""
    )]
    #[case::ctrl_k_at_the_very_end_does_nothing(SubmitKey::CtrlD, "ab⇟x⌁", "\"abx\"")]
    #[case::left_crosses_a_newline(SubmitKey::CtrlD, "a\nb⇤←x⌁", "\"ax\\nb\"")]
    fn a_multiline_edits_and_ends_by_its_rule(
        #[case] submit: SubmitKey,
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let result = multiline("Notes?").submit(submit).simulate(keys(spec));
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[rstest]
    #[case::double_enter(SubmitKey::Enter, "\n\n", "\"x\\ny\"")]
    #[case::ctrl_d(SubmitKey::CtrlD, "⌁", "\"x\\ny\"")]
    fn initial_text_can_have_lines(
        #[case] submit: SubmitKey,
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let result = multiline("Notes?")
            .initial("x\ny")
            .submit(submit)
            .simulate(keys(spec));
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[test]
    fn validate_keeps_the_question_open_under_a_warning() -> Result<()> {
        let ui = live("a\n\nb\n\n");
        let prompt = multiline("Notes?").validate(|text| {
            if text.contains('b') {
                Ok(())
            } else {
                Err("mention b")
            }
        });
        assert_eq!(ui.ask(prompt)?, "ab");
        let frames = ui.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains("▲  Notes?\n")),
            "{frames:?}"
        );
        assert!(
            frames.iter().any(|frame| frame.contains("└  mention b")),
            "{frames:?}"
        );
        Ok(())
    }

    #[test]
    fn parse_reads_the_lines_as_a_value() {
        let number = multiline("Count?").parse::<u32>();
        assert_eq!(number.simulate(keys("42\n\n")).ok(), Some(42));
        let refused = multiline("Count?").parse::<u32>().simulate(keys("x\n\n"));
        assert!(matches!(refused, Err(Error::Refused { .. })), "{refused:?}");
    }

    #[test]
    fn peek_reads_the_text_without_the_confirming_newline() {
        let prompt = multiline("Notes?");
        let state = after(&prompt, keys("a\n\n"));
        let peeked = state.as_ref().and_then(|state| prompt.peek(state));
        assert_eq!(peeked, Some("a"));
    }

    #[rstest]
    #[case::double_enter(SubmitKey::Enter)]
    #[case::ctrl_d(SubmitKey::CtrlD)]
    fn a_line_terminal_gives_one_line(#[case] submit: SubmitKey) {
        let form = bare(Script::new(TerminalKind::Dumb, [], ["hello world"]));
        let result = form.ask(multiline("Notes?").submit(submit));
        assert_eq!(outcome(&result), "\"hello world\"");
        assert_eq!(
            form.terminal().written(),
            "◆  Notes?\n◇  Notes?\n│  hello world\n│\n"
        );
    }

    #[test]
    fn a_piped_multiline_has_no_input() {
        let form = bare(Script::new(TerminalKind::Redirected, [], ["text"]));
        let result = form.ask(multiline("Notes?"));
        assert_eq!(outcome(&result), "no input");
    }

    #[test]
    fn every_line_is_on_the_rail_with_the_caret_and_the_keys() {
        let rows = rendered("hi\nyo", Phase::Active, ColorChoice::Plain);
        assert_eq!(
            rows,
            [
                "│  hi",
                "│  yo▏",
                "│  Enter: new line • Enter on an empty",
                "│  line: submit"
            ]
        );
        let form = live("hi\nyo␛");
        assert!(form.ask(multiline("Notes?")).is_err());
        let frames = form.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains(
                "◆  Notes?\n│  hi\n│  yo▏\n│  Enter: new line • Enter on an empty line: submit\n└"
            )),
            "{frames:?}"
        );
    }

    #[test]
    fn the_ctrl_d_footer_names_its_key() {
        let form = live("␛");
        let result = form.ask(multiline("Notes?").submit(SubmitKey::CtrlD));
        assert!(result.is_err());
        let written = form.terminal().written();
        assert!(
            written.contains("│  Enter: new line • Ctrl-D: submit\n"),
            "{written:?}"
        );
    }

    #[test]
    fn the_placeholder_shows_until_something_is_typed() {
        let form = live("a␛");
        let result = form.ask(multiline("Notes?").placeholder("What changed?"));
        assert!(result.is_err());
        let frames = form.terminal().frames();
        let shows = |row: &str| frames.iter().any(|frame| frame.contains(row));
        assert!(shows("│  ▏What changed?\n"), "{frames:?}");
        assert!(shows("│  a▏\n"), "{frames:?}");
    }

    #[test]
    fn the_submitted_answer_keeps_every_line_on_the_rail() -> Result<()> {
        let form = live("a\nb\n\n");
        form.ask(multiline("Notes?"))?;
        let written = screen(&form.terminal().written());
        assert!(written.ends_with("◇  Notes?\n│  a\n│  b\n│"), "{written}");
        Ok(())
    }

    #[test]
    fn a_cancelled_answer_keeps_every_typed_line() {
        let form = live("a\nb␛");
        let result = form.ask(multiline("Notes?"));
        assert!(matches!(result, Err(Error::Cancelled)));
        let shown = screen(&form.terminal().written());
        assert!(shown.ends_with("■  Notes?\n│  a\n│  b\n│"), "{shown}");
    }

    #[test]
    fn the_rows_repaint_over_each_other() {
        let form = live("a\n\n");
        assert!(form.ask(multiline("Notes?")).is_ok());
        let written = form.terminal().written();
        assert!(written.contains("\r\x1b[5A\x1b[J"), "{written:?}");
    }

    #[test]
    fn colour_inverts_the_character_under_the_caret() {
        let script = Script::new(TerminalKind::Live, keys("ab←␛"), []);
        let form = Session::new(script, Settings::new(ColorChoice::Colored));
        assert!(form.ask(multiline("Notes?")).is_err());
        let written = form.terminal().written();
        assert!(written.contains("a\x1b[7mb\x1b[0m"), "{written:?}");
    }
}
