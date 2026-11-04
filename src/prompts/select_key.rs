//! One option out of several, picked by its own key.

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, WhenSingle},
        error::{Error, Result},
    },
    prompts::choice::{Choice, Options, rail},
    render::{color::ColorChoice, messages::Messages},
    values::line::Cancellation,
};

/// Whether `a` and `A` are the same key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Case {
    /// `a` and `A` are two keys.
    Sensitive,
    /// `a` and `A` are one key.
    #[default]
    Insensitive,
}

impl Case {
    fn same(self, key: char, typed: char) -> bool {
        match self {
            Self::Sensitive => key == typed,
            Self::Insensitive => key.to_lowercase().eq(typed.to_lowercase()),
        }
    }
}

/// Starts a question answered by one key press. Without options it is no prompt
/// yet.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let action: &str = ui.ask(
///     select_key("What next?")
///         .item('r', "run", "Run it")
///         .item('q', "quit", "Quit")
///         .hint("keeps the files")
///         .case(Case::Insensitive),
/// )?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn select_key(message: impl Into<String>) -> EmptySelectKey {
    EmptySelectKey {
        message: message.into(),
    }
}

/// A [`select_key`] without options.
#[derive(Debug)]
pub struct EmptySelectKey {
    message: String,
}

impl EmptySelectKey {
    /// The first option, picked by `key`.
    pub fn item<V>(
        self,
        key: char,
        value: V,
        label: impl Into<String>,
    ) -> SelectKey<V> {
        SelectKey {
            message: self.message,
            options: Options::new(),
            keys: Vec::new(),
            case: Case::default(),
            cursor: None,
            single: WhenSingle::default(),
        }
        .item(key, value, label)
    }
}

/// A question with one answer, picked by key, out of the options added so far.
#[derive(Debug)]
pub struct SelectKey<V> {
    message: String,
    options: Options<V>,
    keys: Vec<char>,
    case: Case,
    cursor: Option<usize>,
    single: WhenSingle,
}

impl<V> SelectKey<V> {
    /// Adds an option that `key` picks. Two options with the same key fail with
    /// [`Error::DuplicateKey`] when it runs.
    #[must_use]
    pub fn item(mut self, key: char, value: V, label: impl Into<String>) -> Self {
        self.keys.push(key);
        Self {
            options: self.options.extend([Choice::new(value, label)]),
            ..self
        }
    }

    /// Shows dim after the last option; empty is none.
    #[must_use]
    pub fn hint(self, hint: impl Into<String>) -> Self {
        Self {
            options: self.options.hint(hint),
            ..self
        }
    }

    /// Whether `a` and `A` are the same key; they are by default.
    #[must_use]
    pub fn case(self, case: Case) -> Self {
        Self { case, ..self }
    }

    /// The option Enter takes, counted from 0; the first by default, the last
    /// when that is past the end.
    #[must_use]
    pub fn default_at(self, index: usize) -> Self {
        Self {
            cursor: Some(index),
            ..self
        }
    }

    /// What to do when there is exactly one option; ask by default.
    #[must_use]
    pub fn skip_if_single(self) -> Self {
        Self {
            single: WhenSingle::Skip,
            ..self
        }
    }

    fn find(&self, typed: char) -> Option<usize> {
        self.keys.iter().position(|key| self.case.same(*key, typed))
    }

    fn repeated(&self) -> Option<char> {
        self.keys.iter().enumerate().find_map(|(at, key)| {
            self.keys
                .iter()
                .take(at)
                .any(|earlier| self.case.same(*earlier, *key))
                .then_some(*key)
        })
    }
}

impl<V> Prompt for SelectKey<V> {
    type Output = V;
    type State = usize;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<usize> {
        let last = self
            .options
            .entries
            .len()
            .checked_sub(1)
            .ok_or(Error::NoChoices)?;
        if let Some(key) = self.repeated() {
            return Err(Error::DuplicateKey(key));
        }
        Ok(self.cursor.unwrap_or(0).min(last))
    }

    fn update(&self, cursor: usize, key: Key) -> Next<usize> {
        match key {
            Key::Char(typed) => {
                self.find(typed).map_or(Next::Editing(cursor), Next::Submit)
            }
            Key::Enter => Next::Submit(cursor),
            Key::Interrupt => Next::Quit(cursor, Cancellation::Interrupt),
            Key::Cancel | Key::EndOfInput => Next::Quit(cursor, Cancellation::Cancel),
            Key::Up
            | Key::Down
            | Key::Tab
            | Key::Left
            | Key::Right
            | Key::Backspace
            | Key::Delete
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(cursor),
        }
    }

    fn view(&self, cursor: &usize, phase: Phase<'_>, frame: &mut Frame) {
        let skin = frame.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let bar = rail(skin, phase);
                let rows: Vec<String> = self
                    .keys
                    .iter()
                    .zip(&self.options.entries)
                    .enumerate()
                    .map(|(at, (key, option))| {
                        let here = at == *cursor;
                        let badge = format!(" {key} ");
                        let (lead, badge) = match (skin.color(), here) {
                            (ColorChoice::Plain, true) => {
                                (format!(" {}", symbols.cursor), badge)
                            }
                            (ColorChoice::Plain, false) => ("  ".to_owned(), badge),
                            (ColorChoice::Colored, true) => (
                                "  ".to_owned(),
                                skin.paint(
                                    theme.active,
                                    &skin.paint(theme.cursor, &badge),
                                ),
                            ),
                            (ColorChoice::Colored, false) => {
                                ("  ".to_owned(), skin.paint(theme.cursor, &badge))
                            }
                        };
                        format!(
                            "{bar}{lead}{badge} {}{}",
                            option.label,
                            option.hinted(|hint| skin.paint(theme.hint, hint))
                        )
                    })
                    .collect();
                frame.extend(rows);
            }
            Phase::Submitted | Phase::Cancelled => {
                frame.row(self.options.label(*cursor));
            }
        }
    }

    fn finish(self, cursor: usize) -> Result<V> {
        self.options
            .values
            .into_iter()
            .nth(cursor)
            .ok_or(Error::NoChoices)
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        self.keys
            .iter()
            .zip(&self.options.entries)
            .map(|(key, option)| {
                format!("  {key}) {}{}", option.label, option.hinted(str::to_owned))
            })
            .chain([messages.listing_key.to_string()])
            .collect()
    }

    /// An empty line takes the highlighted option, a key picks its option, else
    /// cancels.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        match line.trim().chars().next() {
            None => vec![Key::Enter],
            Some(typed) if self.find(typed).is_some() => vec![Key::Char(typed)],
            Some(_) => vec![Key::Cancel],
        }
    }

    fn by_default(&self) -> Option<usize> {
        self.cursor
            .zip(self.options.entries.len().checked_sub(1))
            .map(|(index, last)| index.min(last))
    }

    fn sole(&self) -> Option<(WhenSingle, usize)> {
        (self.options.entries.len() == 1).then_some((self.single, 0))
    }
}

impl<V> Peek for SelectKey<V> {
    type Peeked<'a>
        = &'a V
    where
        Self: 'a;

    fn peek<'a>(&'a self, cursor: &'a usize) -> Option<&'a V> {
        self.options.values.get(*cursor)
    }
}

impl<V: PartialEq> Prefill for SelectKey<V> {
    fn shown_for(&self, answer: &V, _messages: &Messages) -> Option<String> {
        self.options
            .values
            .iter()
            .position(|value| value == answer)
            .map(|at| self.options.label(at).to_owned())
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use super::{Case, SelectKey, select_key};
    use crate::{
        core::{
            ask::{Frame, Key, Phase, Prompt},
            error::{Error, Result},
        },
        fake::{
            Script,
            screen,
            testing::{self, bare},
        },
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    /// Not `Clone` and not `Eq`: the value must move out.
    #[derive(Debug)]
    struct Plan(&'static str);

    fn plans(case: Case) -> SelectKey<Plan> {
        select_key("Which?")
            .item('a', Plan("alpha"), "Plans A")
            .item('B', Plan("beta"), "Plans B")
            .hint("second")
            .item('c', Plan("gamma"), "Plans C")
            .case(case)
    }

    fn chosen(result: &Result<Plan>) -> String {
        testing::outcome(result, |plan| plan.0.to_owned())
    }

    #[rstest]
    #[case::key(Case::Insensitive, TerminalKind::Live, vec![Key::Char('c')], &[], "gamma")]
    #[case::capital_when_insensitive(Case::Insensitive, TerminalKind::Live, vec![Key::Char('A')], &[], "alpha")]
    #[case::lower_for_a_capital_key(Case::Insensitive, TerminalKind::Live, vec![Key::Char('b')], &[], "beta")]
    #[case::sensitive_capital_key(Case::Sensitive, TerminalKind::Live, vec![Key::Char('B')], &[], "beta")]
    #[case::sensitive_other_case_is_ignored(Case::Sensitive, TerminalKind::Live, vec![Key::Char('b'), Key::Char('A'), Key::Char('a')], &[], "alpha")]
    #[case::other_keys_wait(Case::Insensitive, TerminalKind::Live, vec![Key::Char('x'), Key::Down, Key::Char('c')], &[], "gamma")]
    #[case::enter_takes_the_highlighted(Case::Insensitive, TerminalKind::Live, vec![Key::Enter], &[], "alpha")]
    #[case::ctrl_c(Case::Insensitive, TerminalKind::Live, vec![Key::Interrupt], &[], "interrupted")]
    #[case::escape(Case::Insensitive, TerminalKind::Live, vec![Key::Cancel], &[], "cancelled")]
    #[case::line_key(Case::Insensitive, TerminalKind::Dumb, vec![], &["c"], "gamma")]
    #[case::line_first_letter(Case::Insensitive, TerminalKind::Dumb, vec![], &["Beta"], "beta")]
    #[case::line_empty_takes_the_highlighted(Case::Insensitive, TerminalKind::Dumb, vec![], &[""], "alpha")]
    #[case::line_unknown_key(Case::Insensitive, TerminalKind::Dumb, vec![], &["z"], "cancelled")]
    #[case::line_end_of_input(Case::Insensitive, TerminalKind::Dumb, vec![], &[], "cancelled")]
    #[case::piped(Case::Insensitive, TerminalKind::Redirected, vec![], &["a"], "no input")]
    fn a_key_answers_with_its_value(
        #[case] case: Case,
        #[case] kind: TerminalKind,
        #[case] strokes: Vec<Key>,
        #[case] lines: &'static [&'static str],
        #[case] expected: &str,
    ) {
        let ui = bare(Script::new(kind, strokes, lines.iter().copied()));
        let result = ui.ask(plans(case));
        assert_eq!(chosen(&result), expected, "{result:?}");
    }

    #[test]
    fn two_options_with_one_key_are_a_typed_error_and_no_question() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Char('a')], []));
        let result =
            ui.ask(select_key("Which?").item('a', 1, "One").item('A', 2, "Two"));
        assert!(
            matches!(result, Err(Error::DuplicateKey('A'))),
            "{result:?}"
        );
        assert_eq!(ui.terminal().written(), "");
    }

    #[test]
    fn the_same_letter_in_two_cases_is_free_when_sensitive() {
        let value = select_key("Which?")
            .item('a', 1, "One")
            .item('A', 2, "Two")
            .case(Case::Sensitive)
            .simulate([Key::Char('A')]);
        assert_eq!(value.ok(), Some(2));
    }

    #[test]
    fn default_at_decides_what_enter_takes() {
        let value = select_key("Which?")
            .item('a', 1, "One")
            .item('b', 2, "Two")
            .default_at(1)
            .simulate([Key::Enter]);
        assert_eq!(value.ok(), Some(2));
    }

    #[test]
    fn the_rows_show_every_key_and_hint_and_the_summary_stays() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Char('b')], []));
        ui.ask(
            select_key("Which?")
                .item('a', 1, "Plans A")
                .hint("first")
                .item('b', 2, "Plans B"),
        )?;
        let written = ui.terminal().written();
        assert!(
            written.contains("◆  Which?\n│ › a  Plans A (first)\n│   b  Plans B\n└\n"),
            "{written:?}"
        );
        let shown = screen(&written);
        assert!(shown.ends_with("◇  Which?\n│  Plans B\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn a_line_select_key_lists_the_keys() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["b"]));
        let value = ui.ask(
            select_key("Which?")
                .item('a', 1, "Plans A")
                .hint("first")
                .item('b', 2, "Plans B"),
        )?;
        assert_eq!(value, 2);
        assert_eq!(
            ui.terminal().written(),
            "◆  Which?\n  a) Plans A (first)\n  b) Plans B\n  a key picks; Enter takes the default\n◇  Which?\n│  Plans B\n│\n"
        );
        Ok(())
    }

    #[test]
    fn a_cancelled_select_key_shows_the_highlighted_struck() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Cancel], []));
        let result = ui.ask(
            select_key("Pick")
                .item('a', 1, "A")
                .item('b', 2, "B")
                .default_at(1),
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("■  Pick\n│  B\n│"), "{shown}");
    }

    #[test]
    fn colored_badges_are_reversed_and_the_highlighted_one_is_cyan() {
        let prompt = select_key("Which?").item('o', 1, "One").item('T', 2, "Two");
        let mut frame = Frame::new(ColorChoice::Colored, Some(80));
        prompt.view(&0, Phase::Active, &mut frame);
        assert_eq!(
            frame.rows(),
            [
                "\x1b[36m│\x1b[0m  \x1b[36m\x1b[7m o \x1b[0m\x1b[0m One",
                "\x1b[36m│\x1b[0m  \x1b[7m T \x1b[0m Two"
            ]
        );
    }
}
