//! One option out of several, with the value it stands for.

use std::num::NonZeroUsize;

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, WhenSingle},
        error::{Error, Result},
        keys::Printable,
    },
    prompts::choice::{
        ByEq,
        ByLabel,
        CHOICE_ROWS,
        Choice,
        Options,
        Same,
        radio_option,
        rail,
        refused_disabled,
        scrolled,
    },
    render::{
        draw,
        messages::{Messages, Refusal},
    },
    values::line::Cancellation,
};

/// Starts a question with one answer.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let port: u16 = ui.ask(
///     select("Which port?")
///         .item(8080, "Development")
///         .item(443, "Production")
///         .hint("needs a certificate"),
/// )?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn select(message: impl Into<String>) -> EmptySelect {
    EmptySelect {
        message: message.into(),
    }
}

/// A [`select`] without options.
#[derive(Debug)]
pub struct EmptySelect {
    message: String,
}

impl EmptySelect {
    /// The first option.
    pub fn item<V>(self, value: V, label: impl Into<String>) -> Select<V> {
        self.items([Choice::new(value, label)])
    }

    /// The first option, disabled: shown, but never under the cursor.
    pub fn disabled<V>(self, value: V, label: impl Into<String>) -> Select<V> {
        self.items([Choice::disabled(value, label)])
    }

    /// The first options; they may be none, which fails the question with
    /// [`Error::NoChoices`] when it runs.
    pub fn items<V, C: Into<Choice<V>>>(
        self,
        choices: impl IntoIterator<Item = C>,
    ) -> Select<V> {
        Select {
            message: self.message,
            options: Options::new(),
            cursor: None,
            taken: None,
            single: WhenSingle::default(),
            rows: CHOICE_ROWS,
            same: ByEq,
        }
        .items(choices)
    }
}

/// A question with one answer out of the options added so far.
#[derive(Debug)]
pub struct Select<V, S = ByEq> {
    message: String,
    options: Options<V>,
    cursor: Option<usize>,
    /// The option a question takes under `Defaults::Take`, where it is not the
    /// one the cursor starts on.
    taken: Option<usize>,
    single: WhenSingle,
    rows: usize,
    same: S,
}

impl<V> Select<V> {
    pub(crate) fn by_label(
        self,
        label_of: fn(&V) -> &'static str,
    ) -> Select<V, ByLabel<V>> {
        self.same_as(ByLabel(label_of))
    }
}

impl<V, S> Select<V, S> {
    pub(crate) fn same_as<T: Same<V>>(self, same: T) -> Select<V, T> {
        Select {
            message: self.message,
            options: self.options,
            cursor: self.cursor,
            taken: self.taken,
            single: self.single,
            rows: self.rows,
            same,
        }
    }

    #[must_use]
    pub(crate) fn taken_at(self, index: usize) -> Self {
        Self {
            taken: Some(index),
            ..self
        }
    }
}

impl<V, S> Select<V, S> {
    /// Adds an option.
    #[must_use]
    pub fn item(self, value: V, label: impl Into<String>) -> Self {
        self.items([Choice::new(value, label)])
    }

    /// Adds a disabled option: shown dim and struck through, skipped by the
    /// cursor, and refused when a typed line names it.
    #[must_use]
    pub fn disabled(self, value: V, label: impl Into<String>) -> Self {
        self.items([Choice::disabled(value, label)])
    }

    /// Adds options, `(value, label)` or `(value, label, hint)` as well as
    /// [`Choice`]s.
    #[must_use]
    pub fn items<C: Into<Choice<V>>>(
        self,
        choices: impl IntoIterator<Item = C>,
    ) -> Self {
        Self {
            options: self.options.extend(choices),
            ..self
        }
    }

    /// Shows dim after the last option while the cursor is on it; empty is
    /// none.
    #[must_use]
    pub fn hint(self, hint: impl Into<String>) -> Self {
        Self {
            options: self.options.hint(hint),
            ..self
        }
    }

    /// The option the cursor starts on, counted from 0; the last one when that
    /// is past the end, and the next one that is not disabled when it is.
    #[must_use]
    pub fn default_at(self, index: usize) -> Self {
        Self {
            cursor: Some(index),
            ..self
        }
    }

    /// What to do when exactly one option is not disabled; ask by default.
    #[must_use]
    pub fn skip_if_single(self) -> Self {
        Self {
            single: WhenSingle::Skip,
            ..self
        }
    }

    /// How many options show at once.
    #[must_use]
    pub fn max_rows(self, rows: NonZeroUsize) -> Self {
        Self {
            rows: rows.get(),
            ..self
        }
    }

    fn opening(&self) -> Option<usize> {
        self.options.settle(self.cursor.unwrap_or(0))
    }

    fn named(&self, line: &str) -> Option<usize> {
        let wanted = line.to_lowercase();
        line.parse::<usize>()
            .ok()
            .filter(|number| (1..=self.options.entries.len()).contains(number))
            .map(|number| number.saturating_sub(1))
            .or_else(|| {
                self.options
                    .entries
                    .iter()
                    .position(|option| option.label.to_lowercase() == wanted)
            })
    }
}

impl<V, S> Prompt for Select<V, S> {
    type Output = V;
    type State = usize;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<usize> {
        self.opening().ok_or(Error::NoChoices)
    }

    fn update(&self, cursor: usize, key: Key) -> Next<usize> {
        let count = self.options.entries.len();
        match key {
            Key::Up => {
                Next::Editing(self.options.seek(cursor, count.saturating_sub(1)))
            }
            Key::Down | Key::Tab => Next::Editing(self.options.seek(cursor, 1)),
            Key::Enter => Next::Submit(cursor),
            Key::Interrupt => Next::Quit(cursor, Cancellation::Interrupt),
            Key::Cancel | Key::EndOfInput => Next::Quit(cursor, Cancellation::Cancel),
            Key::Char(_)
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
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let bar = rail(skin, phase);
                frame.extend(
                    scrolled(
                        skin,
                        &bar,
                        &self.options.entries,
                        (*cursor, self.rows),
                        |at, option| {
                            let row = radio_option(skin, at == *cursor, option);
                            format!("{bar}  {row}")
                        },
                    )
                    .into_iter()
                    .chain(draw::instructions(
                        skin,
                        &frame.messages().keys_select,
                        frame.width(),
                    )),
                );
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

    fn printable(&self) -> Printable {
        Printable::Bound
    }

    fn listing(&self, _messages: &Messages) -> Vec<String> {
        self.options
            .entries
            .iter()
            .zip(1_usize..)
            .map(|(option, number)| {
                format!(
                    "  {number}) {}{}{}",
                    option.dash(),
                    option.label,
                    option.hinted(str::to_owned)
                )
            })
            .collect()
    }

    /// An empty line takes the highlighted option, a number or a label picks
    /// it.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        let line = line.trim();
        if line.is_empty() {
            return vec![Key::Enter];
        }
        match (self.named(line), self.opening()) {
            (Some(at), Some(from)) => {
                std::iter::repeat_n(Key::Down, self.options.presses(from, at))
                    .chain([Key::Enter])
                    .collect()
            }
            _ => vec![Key::Cancel],
        }
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let Some(at) = self.named(line) else {
            return Some(Refusal::Said(messages.not_an_answer.to_string()));
        };
        refused_disabled(
            &[at.saturating_add(1)],
            |at| self.options.open(at),
            messages,
        )
    }

    fn by_default(&self) -> Option<usize> {
        self.taken
            .or(self.cursor)
            .and_then(|at| self.options.settle(at))
    }

    fn sole(&self) -> Option<(WhenSingle, usize)> {
        let mut open = self.options.open_places();
        open.next()
            .filter(|_| open.next().is_none())
            .map(|at| (self.single, at))
    }
}

impl<V, S> Peek for Select<V, S> {
    type Peeked<'a>
        = &'a V
    where
        Self: 'a;

    fn peek<'a>(&'a self, cursor: &'a usize) -> Option<&'a V> {
        self.options.values.get(*cursor)
    }
}

impl<V, S: Same<V>> Prefill for Select<V, S> {
    fn shown_for(&self, answer: &V, _messages: &Messages) -> Option<String> {
        self.options
            .values
            .iter()
            .enumerate()
            .find(|(at, value)| self.options.open(*at) && self.same.same(value, answer))
            .map(|(at, _)| self.options.label(at).to_owned())
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::num::NonZeroUsize;

    use rstest::rstest;

    use crate::{
        core::{
            ask::{Frame, Key, Phase, Prefill, Prompt},
            error::{Error, Result},
        },
        fake::{
            Script,
            screen,
            testing::{self, bare},
        },
        prompts::{
            choice::Choice,
            select::{Select, select},
        },
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
    };

    const KEYS: &str = "│  ↑/↓ to navigate • Enter: confirm";

    /// Not `Clone` and not `Eq`: the value must move out.
    #[derive(Debug)]
    struct Plan(&'static str);

    fn plans() -> Select<Plan> {
        select("Which?")
            .item(Plan("a"), "Plans A")
            .item(Plan("b"), "Plans B")
            .hint("second")
            .item(Plan("c"), "Plans C")
    }

    fn chosen(result: &Result<Plan>) -> String {
        testing::outcome(result, |plan| plan.0.to_owned())
    }

    #[rstest]
    #[case::enter(vec![Key::Enter], "a")]
    #[case::down_enter(vec![Key::Down, Key::Enter], "b")]
    #[case::j_and_tab(vec![Key::Char('j'), Key::Tab, Key::Enter], "c")]
    #[case::down_wraps(vec![Key::Down, Key::Down, Key::Down, Key::Enter], "a")]
    #[case::up_wraps(vec![Key::Up, Key::Up, Key::Enter], "b")]
    #[case::k_goes_up(vec![Key::Down, Key::Char('k'), Key::Enter], "a")]
    #[case::other_keys_do_nothing(
        vec![Key::Char('x'), Key::Left, Key::Right, Key::Backspace, Key::Ignored, Key::Enter],
        "a"
    )]
    #[case::ctrl_c(vec![Key::Interrupt], "interrupted")]
    #[case::escape(vec![Key::Cancel], "cancelled")]
    #[case::ctrl_d(vec![Key::EndOfInput], "cancelled")]
    #[case::keys_run_out(vec![Key::Down], "no input")]
    fn simulated_keys_answer_with_the_value_of_the_option(
        #[case] keys: Vec<Key>,
        #[case] expected: &str,
    ) {
        let result = plans().simulate(keys);
        assert_eq!(chosen(&result), expected, "{result:?}");
    }

    #[test]
    fn one_option_stays_under_the_cursor() {
        let one = select("One").item(1, "Only");
        assert_eq!(one.simulate([Key::Down, Key::Up, Key::Enter]).ok(), Some(1));
    }

    #[rstest]
    #[case::keys(TerminalKind::Live, vec![Key::Down, Key::Enter], &[], "b")]
    #[case::ctrl_c(TerminalKind::Live, vec![Key::Interrupt], &[], "interrupted")]
    #[case::escape(TerminalKind::Live, vec![Key::Cancel], &[], "cancelled")]
    #[case::line_number(TerminalKind::Dumb, vec![], &["2"], "b")]
    #[case::line_last(TerminalKind::Dumb, vec![], &["3"], "c")]
    #[case::line_first_padded(TerminalKind::Dumb, vec![], &[" 1 "], "a")]
    #[case::line_zero_asks_again(TerminalKind::Dumb, vec![], &["0", "2"], "b")]
    #[case::line_out_of_range_asks_again(TerminalKind::Dumb, vec![], &["4", "2"], "b")]
    #[case::line_words_ask_again(TerminalKind::Dumb, vec![], &["B", "2"], "b")]
    #[case::line_end_of_input(TerminalKind::Dumb, vec![], &[], "cancelled")]
    fn a_session_answers_with_the_value_of_the_option(
        #[case] kind: TerminalKind,
        #[case] strokes: Vec<Key>,
        #[case] lines: &'static [&'static str],
        #[case] expected: &str,
    ) {
        let ui = bare(Script::new(kind, strokes, lines.iter().copied()));
        let result = ui.ask(plans());
        assert_eq!(chosen(&result), expected, "{result:?}");
    }

    #[test]
    fn a_session_draws_the_question_and_leaves_the_answer() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Down, Key::Enter], []));
        ui.ask(plans())?;
        assert_eq!(screen(&ui.terminal().written()), "◇  Which?\n│  Plans B\n│");
        Ok(())
    }

    #[test]
    fn a_cancelled_question_leaves_the_struck_option() {
        let ui = bare(Script::new(
            TerminalKind::Live,
            [Key::Down, Key::Cancel],
            [],
        ));
        assert!(ui.ask(plans()).is_err());
        assert_eq!(screen(&ui.terminal().written()), "■  Which?\n│  Plans B\n│");
    }

    #[test]
    fn a_keyless_terminal_lists_the_options_by_number() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["3"]));
        ui.ask(plans())?;
        assert_eq!(
            screen(&ui.terminal().written()),
            "◆  Which?\n  1) Plans A\n  2) Plans B (second)\n  3) Plans C\n◇  Which?\n│  Plans C\n│"
        );
        Ok(())
    }

    #[test]
    fn no_options_make_a_typed_error_and_no_question() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let result = ui.ask(select("Which?").items::<Plan, Choice<Plan>>([]));
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    #[test]
    fn items_take_choices_and_tuples_in_order() -> Result<()> {
        let picked = select("Which?")
            .item(1, "A")
            .items([Choice::new(2, "B"), Choice::new(3, "C")])
            .items([(4, "D"), (5, "E")])
            .items([(6, "F", "hint")])
            .simulate([Key::Up, Key::Enter])?;
        assert_eq!(picked, 6);
        Ok(())
    }

    #[test]
    fn items_of_a_new_select_may_be_tuples() -> Result<()> {
        let picked = select("Which?")
            .items([("a", "A", "first"), ("b", "B", "")])
            .simulate([Key::Down, Key::Enter])?;
        assert_eq!(picked, "b");
        Ok(())
    }

    #[rstest]
    #[case::first(0, 0, 0)]
    #[case::second(1, 0, 1)]
    #[case::past_the_end(9, 0, 2)]
    #[case::moves_from_there(1, 1, 2)]
    fn default_at_puts_the_cursor_on_that_option(
        #[case] default: usize,
        #[case] downs: usize,
        #[case] expected: usize,
    ) -> Result<()> {
        let keys = std::iter::repeat_n(Key::Down, downs).chain([Key::Enter]);
        let picked = select("Which?")
            .items([(0_usize, "A"), (1, "B"), (2, "C")])
            .default_at(default)
            .simulate(keys)?;
        assert_eq!(picked, expected);
        Ok(())
    }

    fn rendered<V>(select: &Select<V>, cursor: usize, phase: Phase<'_>) -> Vec<String> {
        let mut frame = Frame::new(ColorChoice::Plain, Some(80));
        select.view(&cursor, phase, &mut frame);
        frame.rows().to_vec()
    }

    fn five() -> Select<usize> {
        select("Which?")
            .items([(1, "Plans 1"), (2, "Plans 2"), (3, "Plans 3")])
            .hint("")
            .items([(4, "Plans 4"), (5, "Plans 5")])
            .max_rows(NonZeroUsize::MIN.saturating_add(2))
    }

    #[rstest]
    #[case::top(0, &["│  ● Plans 1", "│  ○ Plans 2", "│  ○ Plans 3", "│  …", KEYS])]
    #[case::middle(3, &["│  …", "│  ○ Plans 3", "│  ● Plans 4", "│  ○ Plans 5", KEYS])]
    #[case::bottom(4, &["│  …", "│  ○ Plans 3", "│  ○ Plans 4", "│  ● Plans 5", KEYS])]
    fn the_view_shows_a_window_round_the_cursor(
        #[case] cursor: usize,
        #[case] rows: &[&str],
    ) {
        assert_eq!(rendered(&five(), cursor, Phase::Active), rows);
    }

    #[test]
    fn the_hint_shows_on_the_option_under_the_cursor_only() {
        let hinted = select("Which?")
            .item(1, "Plans 1")
            .item(2, "Plans 2")
            .hint("id-2")
            .item(3, "Plans 3");
        assert_eq!(
            rendered(&hinted, 1, Phase::Active),
            ["│  ○ Plans 1", "│  ● Plans 2 (id-2)", "│  ○ Plans 3", KEYS]
        );
        assert_eq!(
            rendered(&hinted, 0, Phase::Active),
            ["│  ● Plans 1", "│  ○ Plans 2", "│  ○ Plans 3", KEYS]
        );
    }

    #[test]
    fn a_hint_applies_to_the_last_option_and_empty_is_none() {
        let hinted = select("Which?")
            .item(1, "A")
            .hint("one")
            .item(2, "B")
            .hint("two")
            .hint("");
        assert_eq!(
            rendered(&hinted, 0, Phase::Active)
                .into_iter()
                .take(2)
                .collect::<Vec<_>>(),
            ["│  ● A (one)", "│  ○ B"]
        );
        assert_eq!(
            rendered(&hinted, 1, Phase::Active)
                .into_iter()
                .take(2)
                .collect::<Vec<_>>(),
            ["│  ○ A", "│  ● B"]
        );
    }

    #[rstest]
    #[case::submitted(Phase::Submitted)]
    #[case::cancelled(Phase::Cancelled)]
    fn a_finished_question_shows_the_label_of_the_option(#[case] phase: Phase<'_>) {
        assert_eq!(rendered(&five(), 1, phase), ["Plans 2"]);
    }

    /// A, B off, C, D off.
    fn gaps() -> Select<&'static str> {
        select("Which?")
            .item("a", "Alpha")
            .disabled("b", "Beta")
            .hint("soon")
            .item("c", "Gamma")
            .disabled("d", "Delta")
    }

    #[rstest]
    #[case::down_skips(vec![Key::Down, Key::Enter], "c")]
    #[case::down_wraps_over_the_last(vec![Key::Down, Key::Down, Key::Enter], "a")]
    #[case::up_wraps_over_the_last(vec![Key::Up, Key::Enter], "c")]
    #[case::up_skips(vec![Key::Down, Key::Up, Key::Enter], "a")]
    fn the_cursor_skips_disabled_options_and_wraps(
        #[case] keys: Vec<Key>,
        #[case] expected: &str,
    ) -> Result<()> {
        assert_eq!(gaps().simulate(keys)?, expected);
        Ok(())
    }

    #[rstest]
    #[case::on_a_disabled_one(1, "c")]
    #[case::on_an_open_one(2, "c")]
    #[case::wrapping_past_the_end(3, "a")]
    #[case::past_the_end(9, "a")]
    fn default_at_a_disabled_option_settles_on_the_next_open_one(
        #[case] default: usize,
        #[case] expected: &str,
    ) -> Result<()> {
        assert_eq!(gaps().default_at(default).simulate([Key::Enter])?, expected);
        Ok(())
    }

    #[test]
    fn only_disabled_options_are_no_choices() {
        let none = select("Which?").disabled(1, "A").disabled(2, "B");
        assert!(matches!(none.start(), Err(Error::NoChoices)));
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let result = ui.ask(select("Which?").disabled(1, "A"));
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    #[rstest]
    #[case::number("2")]
    #[case::label("beta")]
    #[case::label_in_any_case("BETA")]
    #[case::last_number("4")]
    fn a_typed_line_naming_a_disabled_option_is_refused_with_a_text(
        #[case] line: &str,
    ) {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let result = ui.ask(gaps().given_line(Some(line)));
        assert!(
            matches!(&result, Err(Error::Refused { question, reason })
                if question == "Which?" && *reason == Messages::english().option_disabled),
            "{result:?}"
        );
    }

    #[test]
    fn a_keyless_terminal_marks_disabled_options_and_asks_again_after_a_refusal()
    -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2", "3"]));
        assert_eq!(ui.ask(gaps())?, "c");
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.contains(
                "  1) Alpha\n  2) [-] Beta (soon)\n  3) Gamma\n  4) [-] Delta"
            ),
            "{shown}"
        );
        assert!(shown.contains("That option is disabled."), "{shown}");
        Ok(())
    }

    #[test]
    fn typed_numbers_reach_open_options_behind_disabled_ones() -> Result<()> {
        for (line, expected) in [("1", "a"), ("3", "c"), ("gamma", "c")] {
            let ui = bare(Script::new(TerminalKind::Dumb, [], [line]));
            assert_eq!(ui.ask(gaps())?, expected);
        }
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["3"]));
        assert_eq!(ui.ask(gaps().default_at(2))?, "c");
        Ok(())
    }

    #[test]
    fn a_given_disabled_value_is_refused_and_an_open_one_taken() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let refused = ui.ask(gaps().given(Some("b")));
        assert!(
            matches!(&refused, Err(Error::Refused { question, .. }) if question == "Which?"),
            "{refused:?}"
        );
        assert_eq!(ui.ask(gaps().given(Some("c")))?, "c");
        Ok(())
    }

    #[test]
    fn skip_if_single_counts_the_open_options() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let only = select("Which?")
            .disabled(1, "A")
            .item(2, "B")
            .disabled(3, "C")
            .skip_if_single();
        assert_eq!(ui.ask(only)?, 2);
        let asking = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let two = select("Which?")
            .item(1, "A")
            .item(2, "B")
            .disabled(3, "C")
            .skip_if_single();
        assert_eq!(asking.ask(two)?, 1);
        Ok(())
    }

    #[test]
    fn a_disabled_option_looks_dim_and_struck_and_shows_its_hint() {
        let rows = rendered(&gaps(), 0, Phase::Active);
        assert_eq!(
            rows,
            [
                "│  ● Alpha",
                "│  ○ Beta (soon)",
                "│  ○ Gamma",
                "│  ○ Delta",
                KEYS
            ]
        );
        let mut frame = Frame::new(ColorChoice::Colored, Some(80));
        gaps().view(&0, Phase::Active, &mut frame);
        let beta = frame.rows().get(1).cloned().unwrap_or_default();
        assert!(beta.contains("\x1b[2m\x1b[9mBeta\x1b[0m"), "{beta:?}");
        assert!(beta.contains("(soon)"), "{beta:?}");
        let gamma = frame.rows().get(2).cloned().unwrap_or_default();
        assert!(!gamma.contains("\x1b[9m"), "{gamma:?}");
    }
}
