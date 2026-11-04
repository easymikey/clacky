//! Any number of options out of several, with the values they stand for.

use std::num::NonZeroUsize;

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt},
        error::{Error, Result},
        keys::Printable,
    },
    prompts::choice::{
        ByEq,
        ByLabel,
        CHOICE_ROWS,
        Choice,
        EmptyPick,
        Entry,
        Options,
        Same,
        checkbox_row,
        named,
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

/// Starts a question with any number of answers. It has no options yet, so
/// it is no prompt: add them with [`item`](EmptyMultiSelect::item) or
/// [`items`](EmptyMultiSelect::items), which give a [`MultiSelect`].
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let tools: Vec<&str> = ui.ask(
///     multiselect("Which tools?")
///         .item("fmt", "Formatter")
///         .item("lint", "Linter")
///         .hint("slow on big trees")
///         .picked([0])
///         .optional(),
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// A question without options does not compile:
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = multiselect("Kind").simulate([Key::Enter]);
/// ```
#[must_use]
pub fn multiselect(message: impl Into<String>) -> EmptyMultiSelect {
    EmptyMultiSelect {
        message: message.into(),
    }
}

/// A [`multiselect`] without options.
#[derive(Debug)]
pub struct EmptyMultiSelect {
    message: String,
}

impl EmptyMultiSelect {
    /// The first option.
    pub fn item<V>(self, value: V, label: impl Into<String>) -> MultiSelect<V> {
        self.items([Choice::new(value, label)])
    }

    /// The first option, disabled: shown, but never under the cursor.
    pub fn disabled<V>(self, value: V, label: impl Into<String>) -> MultiSelect<V> {
        self.items([Choice::disabled(value, label)])
    }

    /// The first options; they may be none, which fails the question with
    /// [`Error::NoChoices`] when it runs.
    pub fn items<V, C: Into<Choice<V>>>(
        self,
        choices: impl IntoIterator<Item = C>,
    ) -> MultiSelect<V> {
        MultiSelect {
            message: self.message,
            options: Options::new(),
            picked: None,
            empty: EmptyPick::Refused,
            cursor: 0,
            rows: CHOICE_ROWS,
            same: ByEq,
        }
        .items(choices)
    }
}

/// A question with any answers out of the options added so far. Enter with
/// nothing picked is refused unless it is [`optional`](Self::optional).
#[derive(Debug)]
pub struct MultiSelect<V, S = ByEq> {
    message: String,
    options: Options<V>,
    picked: Option<Vec<usize>>,
    empty: EmptyPick,
    cursor: usize,
    rows: usize,
    same: S,
}

impl<V> MultiSelect<V> {
    pub(crate) fn by_label(
        self,
        label_of: fn(&V) -> &'static str,
    ) -> MultiSelect<V, ByLabel<V>> {
        MultiSelect {
            message: self.message,
            options: self.options,
            picked: self.picked,
            empty: self.empty,
            cursor: self.cursor,
            rows: self.rows,
            same: ByLabel(label_of),
        }
    }
}

impl<V, S> MultiSelect<V, S> {
    /// Adds an option.
    #[must_use]
    pub fn item(self, value: V, label: impl Into<String>) -> Self {
        self.items([Choice::new(value, label)])
    }

    /// Adds a disabled option: shown dim and struck through, skipped by the
    /// cursor, never picked by a key (not by `a` or `i` either) and refused
    /// when a typed line names it.
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

    /// The options that start picked, counted from 0; others are ignored, and
    /// so are disabled ones.
    #[must_use]
    pub fn picked(self, indices: impl IntoIterator<Item = usize>) -> Self {
        Self {
            picked: Some(indices.into_iter().collect()),
            ..self
        }
    }

    /// Enter with nothing picked answers with an empty list instead of
    /// being refused.
    #[must_use]
    pub fn optional(self) -> Self {
        Self {
            empty: EmptyPick::Allowed,
            ..self
        }
    }

    /// The option the cursor starts on, counted from 0; the last one when
    /// that is past the end, and the next one that is not disabled when it is.
    #[must_use]
    pub fn cursor_at(self, index: usize) -> Self {
        Self {
            cursor: index,
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
}

/// The cursor, and which rows are picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picks {
    pub(crate) cursor: usize,
    pub(crate) on: Vec<bool>,
}

impl Picks {
    fn toggled(self) -> Self {
        let on = (0..)
            .zip(&self.on)
            .map(|(at, on)| *on != (at == self.cursor))
            .collect();
        Self { on, ..self }
    }

    pub(crate) fn all(self, pickable: impl Fn(usize) -> bool) -> Self {
        let full = (0..).zip(&self.on).all(|(at, on)| *on || !pickable(at));
        let on = (0..)
            .zip(&self.on)
            .map(|(at, on)| if pickable(at) { !full } else { *on })
            .collect();
        Self { on, ..self }
    }

    pub(crate) fn inverted(self, pickable: impl Fn(usize) -> bool) -> Self {
        let on = (0..)
            .zip(&self.on)
            .map(|(at, on)| *on != pickable(at))
            .collect();
        Self { on, ..self }
    }

    pub(crate) fn none(&self) -> bool {
        !self.on.contains(&true)
    }
}

pub(crate) fn labelled(entries: &[Entry], on: &[bool]) -> Option<String> {
    let labels: Vec<&str> = entries
        .iter()
        .zip(on)
        .filter(|(_, on)| **on)
        .map(|(option, _)| option.label.as_str())
        .collect();
    (!labels.is_empty()).then(|| labels.join(", "))
}

pub(crate) fn picked_labels<V>(
    entries: &[Entry],
    values: &[Option<&V>],
    answer: &[V],
    same: impl Fn(&V, &V) -> bool,
    messages: &Messages,
) -> Option<String> {
    let known = answer.iter().all(|picked| {
        values
            .iter()
            .any(|value| value.is_some_and(|value| same(value, picked)))
    });
    let on: Vec<bool> = values
        .iter()
        .map(|value| {
            value.is_some_and(|value| answer.iter().any(|picked| same(value, picked)))
        })
        .collect();
    known.then(|| labelled(entries, &on).unwrap_or_else(|| messages.none.to_string()))
}

pub(crate) fn walk(
    start: &Picks,
    named: &[usize],
    rests: usize,
    (rank, step): (impl Fn(usize) -> Option<usize>, impl Fn(usize) -> usize),
) -> Vec<Key> {
    let (_, keys) = (0..rests).fold((start.cursor, Vec::new()), |(cursor, keys), _| {
        let flip = rank(cursor).is_some_and(|rank| {
            start
                .on
                .get(cursor)
                .is_some_and(|on| *on != named.contains(&rank.saturating_add(1)))
        });
        let keys: Vec<Key> = keys
            .into_iter()
            .chain(flip.then_some(Key::Char(' ')))
            .chain([Key::Down])
            .collect();
        (step(cursor), keys)
    });
    keys.into_iter().chain([Key::Enter]).collect()
}

impl<V, S> MultiSelect<V, S> {
    fn count(&self) -> usize {
        self.options.entries.len()
    }

    fn opening(&self) -> Option<Picks> {
        Some(Picks {
            cursor: self.options.settle(self.cursor)?,
            on: (0..self.count())
                .map(|at| {
                    self.options.open(at)
                        && self
                            .picked
                            .as_deref()
                            .is_some_and(|picked| picked.contains(&at))
                })
                .collect(),
        })
    }

    fn pickable(&self, at: usize) -> bool {
        self.options.open(at)
    }
}

impl<V, S> Prompt for MultiSelect<V, S> {
    type Output = Vec<V>;
    type State = Picks;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Picks> {
        self.opening().ok_or(Error::NoChoices)
    }

    fn update(&self, state: Picks, key: Key) -> Next<Picks> {
        let count = self.count();
        let moved = |from: Picks, step: usize| Picks {
            cursor: self.options.seek(from.cursor, step),
            ..from
        };
        match key {
            Key::Up => Next::Editing(moved(state, count.saturating_sub(1))),
            Key::Down | Key::Tab => Next::Editing(moved(state, 1)),
            Key::Char(' ') => Next::Editing(state.toggled()),
            Key::Char('a') => Next::Editing(state.all(|at| self.pickable(at))),
            Key::Char('i') => Next::Editing(state.inverted(|at| self.pickable(at))),
            Key::Enter if self.empty == EmptyPick::Refused && state.none() => {
                Next::Refuse(state, Refusal::NoOption)
            }
            Key::Enter => Next::Submit(state),
            Key::Interrupt => Next::Quit(state, Cancellation::Interrupt),
            Key::Cancel | Key::EndOfInput => Next::Quit(state, Cancellation::Cancel),
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
            | Key::Ignored => Next::Editing(state),
        }
    }

    fn view(&self, state: &Picks, phase: Phase<'_>, frame: &mut Frame) {
        let skin = frame.skin();
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let bar = rail(skin, phase);
                frame.extend(
                    scrolled(
                        skin,
                        &bar,
                        &self.options.entries,
                        (state.cursor, self.rows),
                        |at, option| {
                            checkbox_row(
                                skin,
                                &bar,
                                (at == state.cursor, state.on.get(at) == Some(&true)),
                                option,
                            )
                        },
                    )
                    .into_iter()
                    .chain(draw::instructions(
                        skin,
                        &frame.messages().keys_multiselect,
                        frame.width(),
                    )),
                );
            }
            Phase::Submitted => frame.row(
                labelled(&self.options.entries, &state.on)
                    .unwrap_or_else(|| frame.messages().none.to_string()),
            ),
            Phase::Cancelled => {
                frame.extend(labelled(&self.options.entries, &state.on));
            }
        }
    }

    fn finish(self, state: Picks) -> Result<Vec<V>> {
        Ok(self
            .options
            .values
            .into_iter()
            .zip(state.on)
            .filter_map(|(value, on)| on.then_some(value))
            .collect())
    }

    fn printable(&self) -> Printable {
        Printable::Bound
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        let marked = self.opening().map(|picks| picks.on).unwrap_or_default();
        let rows = (1_usize..)
            .zip(self.options.entries.iter().zip(marked))
            .map(|(number, (option, on))| {
                format!(
                    "  {number}) {} {}{}",
                    option.tick(on),
                    option.label,
                    option.hinted(str::to_owned)
                )
            });
        rows.chain([messages.listing_numbers.to_string()]).collect()
    }

    fn by_default(&self) -> Option<Picks> {
        self.picked.as_ref().and_then(|_| self.opening())
    }

    /// Numbers name the options to pick; none keeps the marked; else cancels.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        let (Some(named), Some(start)) = (named(line, self.count()), self.opening())
        else {
            return vec![Key::Cancel];
        };
        if named.is_empty() {
            return vec![Key::Enter];
        }
        walk(
            &start,
            &named,
            self.options.open_places().count(),
            (Some, |cursor| self.options.seek(cursor, 1)),
        )
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        let named = named(line, self.count())?;
        refused_disabled(&named, |at| self.pickable(at), messages)
    }
}

impl<V, S> Peek for MultiSelect<V, S> {
    type Peeked<'a>
        = Vec<&'a V>
    where
        Self: 'a;

    fn peek<'a>(&'a self, state: &'a Picks) -> Option<Vec<&'a V>> {
        Some(
            self.options
                .values
                .iter()
                .zip(&state.on)
                .filter_map(|(value, on)| on.then_some(value))
                .collect(),
        )
    }
}

impl<V, S: Same<V>> Prefill for MultiSelect<V, S> {
    fn shown_for(&self, answer: &Vec<V>, messages: &Messages) -> Option<String> {
        let values: Vec<Option<&V>> = (0..)
            .zip(&self.options.values)
            .map(|(at, value)| self.pickable(at).then_some(value))
            .collect();
        picked_labels(
            &self.options.entries,
            &values,
            answer,
            |a, b| self.same.same(a, b),
            messages,
        )
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

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
            multiselect::{MultiSelect, multiselect},
        },
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
    };

    const SPACE: Key = Key::Char(' ');
    const KEYS: &str = "│  ↑/↓ to navigate • Space: select • a: all • Enter: confirm";

    /// Not `Clone` and not `Eq`: the values must move out.
    #[derive(Debug)]
    struct Plan(&'static str);

    fn plans() -> MultiSelect<Plan> {
        multiselect("Which?")
            .item(Plan("a"), "Plans A")
            .item(Plan("b"), "Plans B")
            .hint("second")
            .item(Plan("c"), "Plans C")
            .picked([1])
            .optional()
    }

    fn picked(result: &Result<Vec<Plan>>) -> String {
        testing::outcome(result, |plans| {
            format!("[{}]", plans.iter().map(|plan| plan.0).collect::<String>())
        })
    }

    #[rstest]
    #[case::pick_beside_the_marked(TerminalKind::Live, vec![SPACE, Key::Down, Key::Down, SPACE, Key::Enter], &[], "[abc]")]
    #[case::option_order_not_toggle_order(TerminalKind::Live, vec![Key::Down, Key::Down, SPACE, Key::Up, Key::Up, SPACE, Key::Enter], &[], "[abc]")]
    #[case::unpick_the_marked(TerminalKind::Live, vec![Key::Down, SPACE, Key::Enter], &[], "[]")]
    #[case::start_picked(TerminalKind::Live, vec![Key::Enter], &[], "[b]")]
    #[case::a_picks_all(TerminalKind::Live, vec![Key::Char('a'), Key::Enter], &[], "[abc]")]
    #[case::a_twice_picks_none(TerminalKind::Live, vec![Key::Char('a'), Key::Char('a'), Key::Enter], &[], "[]")]
    #[case::i_inverts(TerminalKind::Live, vec![Key::Char('i'), Key::Enter], &[], "[ac]")]
    #[case::j_k_tab_move(TerminalKind::Live, vec![Key::Char('j'), Key::Tab, Key::Char('k'), Key::Char('k'), SPACE, Key::Enter], &[], "[ab]")]
    #[case::ctrl_c(TerminalKind::Live, vec![Key::Interrupt], &[], "interrupted")]
    #[case::escape(TerminalKind::Live, vec![Key::Cancel], &[], "cancelled")]
    #[case::ctrl_d(TerminalKind::Live, vec![Key::EndOfInput], &[], "cancelled")]
    #[case::line_numbers(TerminalKind::Dumb, vec![], &["1 3"], "[ac]")]
    #[case::line_commas(TerminalKind::Dumb, vec![], &["1,2"], "[ab]")]
    #[case::line_keeps_the_marked(TerminalKind::Dumb, vec![], &[""], "[b]")]
    #[case::line_words(TerminalKind::Dumb, vec![], &["all"], "cancelled")]
    #[case::line_out_of_range(TerminalKind::Dumb, vec![], &["4"], "cancelled")]
    #[case::line_end_of_input(TerminalKind::Dumb, vec![], &[], "cancelled")]
    fn a_multiselect_answers_with_the_values_in_option_order(
        #[case] kind: TerminalKind,
        #[case] strokes: Vec<Key>,
        #[case] lines: &'static [&'static str],
        #[case] expected: &str,
    ) {
        let ui = bare(Script::new(kind, strokes, lines.iter().copied()));
        let result = ui.ask(plans());
        assert_eq!(picked(&result), expected, "{result:?}");
    }

    #[test]
    fn a_typed_line_names_the_options_whatever_row_the_cursor_starts_on() -> Result<()>
    {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["1 3"]));
        let values = ui.ask(plans().cursor_at(2).picked([]))?;
        assert_eq!(values.iter().map(|plan| plan.0).collect::<String>(), "ac");
        Ok(())
    }

    #[test]
    fn no_options_make_a_typed_error_and_no_question() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let result = ui.ask(multiselect("Which?").items::<Plan, Choice<Plan>>([]));
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    #[test]
    fn an_empty_pick_is_refused_by_default_and_waits_for_a_pick() -> Result<()> {
        let strokes = [Key::Enter, SPACE, Key::Enter];
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        let values = ui.ask(
            multiselect("Which?")
                .item(Plan("a"), "Plans A")
                .item(Plan("b"), "Plans B"),
        )?;
        assert_eq!(values.iter().map(|plan| plan.0).collect::<String>(), "a");
        let written = ui.terminal().written();
        assert_eq!(
            written
                .matches("Please select at least one option.")
                .count(),
            1,
            "{written:?}"
        );
        assert!(written.contains("▲  Which?"), "{written:?}");
        assert!(
            written.contains("└  Please select at least one option."),
            "{written:?}"
        );
        assert!(
            written.contains("◇  Which?\n│  Plans A\n│\n"),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn a_refused_pick_keeps_the_question_open_in_a_simulation() {
        let result = multiselect("Which?").item(1, "A").simulate([Key::Enter]);
        assert!(
            matches!(&result, Err(Error::Refused { reason, .. }) if reason == "Please select at least one option."),
            "{result:?}"
        );
    }

    #[test]
    fn optional_answers_nothing_and_shows_none() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let values = ui.ask(multiselect("Which?").item(1, "A").optional())?;
        assert!(values.is_empty());
        assert!(ui.terminal().written().contains("◇  Which?\n│  none\n│\n"));
        Ok(())
    }

    #[test]
    fn cursor_at_decides_where_space_lands() -> Result<()> {
        let values = multiselect("Which?")
            .items([(1, "A"), (2, "B"), (3, "C")])
            .cursor_at(2)
            .simulate([SPACE, Key::Enter])?;
        assert_eq!(values, [3]);
        Ok(())
    }

    #[test]
    fn picked_indices_start_picked_and_those_past_the_end_are_ignored() -> Result<()> {
        let values = multiselect("Which?")
            .items([(1, "A"), (2, "B"), (3, "C")])
            .picked([2, 7])
            .simulate([Key::Enter])?;
        assert_eq!(values, [3]);
        Ok(())
    }

    fn rendered<V>(
        select: &MultiSelect<V>,
        keys: &[Key],
        phase: Phase<'_>,
    ) -> Vec<String> {
        testing::rendered(select, keys.iter().copied(), phase, ColorChoice::Plain, 80)
    }

    #[test]
    fn the_footer_wraps_under_the_rail_on_a_narrow_terminal() {
        let select = multiselect("Which?").item(1, "Plans A");
        let mut frame = Frame::new(ColorChoice::Plain, Some(30));
        if let Ok(state) = select.start() {
            select.view(&state, Phase::Active, &mut frame);
        }
        let footer: Vec<&String> = frame.rows().iter().skip(1).collect();
        assert!(footer.len() > 1, "{footer:?}");
        assert!(
            footer.iter().all(|row| row.starts_with("│  ")),
            "{footer:?}"
        );
        assert!(footer.iter().all(|row| !row.contains('…')), "{footer:?}");
        let words: String = footer
            .iter()
            .filter_map(|row| row.strip_prefix("│  "))
            .collect();
        assert!(
            words.replace(' ', "").ends_with("a:all•Enter:confirm"),
            "{words}"
        );
    }

    #[test]
    fn the_hint_shows_dim_after_the_cursor_option_only() {
        let hinted = multiselect("Which?")
            .item(1, "Plans A")
            .hint("first")
            .item(2, "Plans B")
            .hint("second");
        assert_eq!(
            rendered(&hinted, &[], Phase::Active),
            ["│ ›◻ Plans A (first)", "│  ◻ Plans B", KEYS]
        );
        assert_eq!(
            rendered(&hinted, &[Key::Down, SPACE], Phase::Active),
            ["│  ◻ Plans A", "│ ›◼ Plans B (second)", KEYS]
        );
    }

    #[test]
    fn a_hint_applies_to_the_last_option_and_empty_is_none() {
        let hinted = multiselect("Which?")
            .item(1, "A")
            .hint("one")
            .item(2, "B")
            .hint("two")
            .hint("");
        assert_eq!(
            rendered(&hinted, &[], Phase::Active),
            ["│ ›◻ A (one)", "│  ◻ B", KEYS]
        );
    }

    #[rstest]
    #[case::top(&[], &["│ ›◻ Plans 1", "│  ◻ Plans 2", "│  ◻ Plans 3", "│  …", KEYS])]
    #[case::middle(&[Key::Down, Key::Down, Key::Down], &["│  …", "│  ◻ Plans 2", "│  ◻ Plans 3", "│ ›◻ Plans 4", "│  …", KEYS])]
    #[case::bottom(&[Key::Up], &["│  …", "│  ◻ Plans 3", "│  ◻ Plans 4", "│ ›◻ Plans 5", KEYS])]
    fn the_view_shows_a_window_round_the_cursor(
        #[case] keys: &[Key],
        #[case] rows: &[&str],
    ) {
        let five = multiselect("Which?")
            .items((1..=5).map(|number| Choice::new(number, format!("Plans {number}"))))
            .max_rows(std::num::NonZeroUsize::MIN.saturating_add(2));
        assert_eq!(rendered(&five, keys, Phase::Active), rows);
    }

    #[test]
    fn the_footer_lists_the_keys_above_the_end_of_the_rail() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [SPACE, Key::Enter], []));
        ui.ask(multiselect("Which?").item(1, "Plans A"))?;
        let written = ui.terminal().written();
        assert!(
            written.contains(
                "│  ↑/↓ to navigate • Space: select • a: all • Enter: confirm\n└"
            ),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn the_active_rows_repaint_and_the_summary_stays() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [SPACE, Key::Enter], []));
        let values = ui.ask(multiselect("Which?").items([
            (1, "Plans A"),
            (2, "Plans B"),
            (3, "Plans C"),
        ]))?;
        assert_eq!(values, [1]);
        let written = ui.terminal().written();
        assert!(
            written.contains("\x1b[5A\r\x1b[2K│ ›◼ Plans A\x1b[5B\r"),
            "{written:?}"
        );
        assert!(written.contains("│ ›◼ Plans A"), "{written:?}");
        Ok(())
    }

    #[test]
    fn a_line_multiselect_lists_the_options_with_marks() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2 3"]));
        let values = ui.ask(
            multiselect("Which?")
                .items([(1, "Plans A"), (2, "Plans B"), (3, "Plans C")])
                .picked([0]),
        )?;
        assert_eq!(values, [2, 3]);
        assert_eq!(
            ui.terminal().written(),
            "◆  Which?\n  1) [x] Plans A\n  2) [ ] Plans B\n  3) [ ] Plans C\n  numbers to pick, e.g. 1 3; Enter keeps the marked\n◇  Which?\n│  Plans B, Plans C\n│\n"
        );
        Ok(())
    }

    #[rstest]
    #[case::two_ticked(vec![SPACE, Key::Down, Key::Down, SPACE], "■  Pick\n│  A, C\n│")]
    #[case::none_ticked(vec![], "■  Pick\n│")]
    fn cancelled_multiselect_lists_the_ticked(
        #[case] keys: Vec<Key>,
        #[case] tail: &str,
    ) {
        let strokes = keys.into_iter().chain([Key::Cancel]);
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        let result = ui.ask(
            multiselect("Pick").items(["A", "B", "C"].map(|label| (label, label))),
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with(tail), "{shown}");
    }

    /// A, B off, C, D off, E.
    fn gaps() -> MultiSelect<&'static str> {
        multiselect("Which?")
            .item("a", "Alpha")
            .disabled("b", "Beta")
            .hint("soon")
            .item("c", "Gamma")
            .disabled("d", "Delta")
            .item("e", "Epsilon")
            .optional()
    }

    #[rstest]
    #[case::down_skips(vec![Key::Down, SPACE, Key::Enter], "c")]
    #[case::down_twice(vec![Key::Down, Key::Down, SPACE, Key::Enter], "e")]
    #[case::down_wraps(vec![Key::Down, Key::Down, Key::Down, SPACE, Key::Enter], "a")]
    #[case::up_wraps(vec![Key::Up, SPACE, Key::Enter], "e")]
    #[case::up_skips(vec![Key::Down, Key::Down, Key::Up, SPACE, Key::Enter], "c")]
    fn the_cursor_skips_disabled_options_and_wraps(
        #[case] keys: Vec<Key>,
        #[case] expected: &str,
    ) -> Result<()> {
        assert_eq!(gaps().simulate(keys)?, [expected]);
        Ok(())
    }

    #[rstest]
    #[case::a_picks_the_open_ones(Key::Char('a'), vec!["a", "c", "e"])]
    #[case::i_inverts_the_open_ones(Key::Char('i'), vec!["a", "c", "e"])]
    fn a_and_i_never_pick_a_disabled_option(
        #[case] key: Key,
        #[case] expected: Vec<&str>,
    ) -> Result<()> {
        assert_eq!(gaps().simulate([key, Key::Enter])?, expected);
        let twice = gaps().simulate([key, key, Key::Enter])?;
        assert!(twice.is_empty(), "{twice:?}");
        Ok(())
    }

    #[test]
    fn picked_and_cursor_at_a_disabled_option_settle_on_open_ones() -> Result<()> {
        assert_eq!(
            gaps().picked([0, 1, 3]).simulate([Key::Enter])?,
            ["a"],
            "disabled starts unpicked"
        );
        assert_eq!(gaps().cursor_at(1).simulate([SPACE, Key::Enter])?, ["c"]);
        assert_eq!(gaps().cursor_at(3).simulate([SPACE, Key::Enter])?, ["e"]);
        assert_eq!(gaps().cursor_at(9).simulate([SPACE, Key::Enter])?, ["e"]);
        Ok(())
    }

    #[test]
    fn only_disabled_options_are_no_choices() {
        let result = multiselect("Which?")
            .disabled(1, "A")
            .disabled(2, "B")
            .simulate([Key::Enter]);
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
    }

    #[test]
    fn a_typed_line_naming_a_disabled_option_is_refused_with_a_text() {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        for line in ["2", "1 4", "3,2"] {
            let result = ui.ask(gaps().given_line(Some(line)));
            assert!(
                matches!(&result, Err(Error::Refused { reason, .. })
                    if *reason == Messages::english().option_disabled),
                "{line}: {result:?}"
            );
        }
    }

    #[test]
    fn a_keyless_terminal_marks_disabled_options_and_walks_past_them() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2", "1 3 5"]));
        assert_eq!(ui.ask(gaps())?, ["a", "c", "e"]);
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.contains(
                "  1) [ ] Alpha\n  2) [-] Beta (soon)\n  3) [ ] Gamma\n  4) [-] Delta\n  5) [ ] Epsilon"
            ),
            "{shown}"
        );
        assert!(shown.contains("That option is disabled."), "{shown}");
        Ok(())
    }

    #[test]
    fn a_typed_line_unpicks_marked_options_behind_disabled_ones() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["3"]));
        assert_eq!(ui.ask(gaps().picked([0, 4]))?, ["c"]);
        Ok(())
    }

    #[test]
    fn a_given_answer_with_a_disabled_value_is_refused() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let refused = ui.ask(gaps().given(Some(vec!["a", "b"])));
        assert!(
            matches!(&refused, Err(Error::Refused { question, .. }) if question == "Which?"),
            "{refused:?}"
        );
        assert_eq!(ui.ask(gaps().given(Some(vec!["a", "e"])))?, ["a", "e"]);
        Ok(())
    }

    #[test]
    fn a_disabled_option_looks_dim_and_struck_and_shows_its_hint() {
        let mut frame = Frame::new(ColorChoice::Plain, Some(80));
        let prompt = gaps();
        if let Ok(state) = prompt.start() {
            prompt.view(&state, Phase::Active, &mut frame);
        }
        assert_eq!(
            frame.rows(),
            [
                "│ ›◻ Alpha",
                "│  ◻ Beta (soon)",
                "│  ◻ Gamma",
                "│  ◻ Delta",
                "│  ◻ Epsilon",
                KEYS
            ]
        );
        let mut colored = Frame::new(ColorChoice::Colored, Some(80));
        if let Ok(state) = prompt.start() {
            prompt.view(&state, Phase::Active, &mut colored);
        }
        let beta = colored.rows().get(1).cloned().unwrap_or_default();
        assert!(beta.contains("\x1b[2m\x1b[9mBeta\x1b[0m"), "{beta:?}");
    }
}
