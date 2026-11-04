//! Any number of options out of several named groups, with the values they
//! stand for.

use std::{iter, num::NonZeroUsize};

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt},
        error::{Error, Result},
        keys::Printable,
    },
    prompts::{
        choice::{
            CHOICE_ROWS,
            Choice,
            EmptyPick,
            Entry,
            Fill,
            checkbox,
            disabled_text,
            named,
            rail,
            refused_disabled,
            scrolled,
            seek,
        },
        multiselect::{Picks, labelled, picked_labels, walk},
    },
    render::{
        color::ColorChoice,
        draw,
        messages::{Messages, Refusal},
        style::Skin,
    },
    values::line::Cancellation,
};

/// What a group's own row does in a [`GroupMultiSelect`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GroupPick {
    /// The group's row can be picked, and picks or drops all its options.
    #[default]
    Whole,
    /// The group's row is a heading: the cursor skips it.
    ItemsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Header(usize),
    Item { group: usize, last: bool },
}

/// Starts a question with any number of answers under group headings. Enter
/// with nothing picked is refused unless
/// [`optional`](GroupMultiSelect::optional).
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let tools: Vec<&str> = ui.ask(
///     group_multiselect("Which tools?")
///         .group("Rust", [("fmt", "Formatter"), ("lint", "Linter")])
///         .group("Web", [("bundle", "Bundler", "slow")])
///         .selectable_groups(GroupPick::ItemsOnly)
///         .optional(),
/// )?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn group_multiselect(message: impl Into<String>) -> EmptyGroupMultiSelect {
    EmptyGroupMultiSelect {
        message: message.into(),
    }
}

/// A [`group_multiselect`] without options.
#[derive(Debug)]
pub struct EmptyGroupMultiSelect {
    message: String,
}

impl EmptyGroupMultiSelect {
    /// The first group of options under the heading `label`. A group without
    /// options is dropped, and none at all fails with [`Error::NoChoices`] when
    /// it runs.
    pub fn group<V, C: Into<Choice<V>>>(
        self,
        label: impl Into<String>,
        choices: impl IntoIterator<Item = C>,
    ) -> GroupMultiSelect<V> {
        GroupMultiSelect {
            message: self.message,
            values: Vec::new(),
            entries: Vec::new(),
            places: Vec::new(),
            picked: None,
            empty: EmptyPick::Refused,
            group_pick: GroupPick::default(),
            cursor: None,
            spacing: 0,
            rows: CHOICE_ROWS,
        }
        .group(label, choices)
    }
}

/// A question with any answers out of the groups added so far.
#[derive(Debug)]
pub struct GroupMultiSelect<V> {
    message: String,
    values: Vec<Option<V>>,
    entries: Vec<Entry>,
    places: Vec<Place>,
    picked: Option<Vec<usize>>,
    empty: EmptyPick,
    group_pick: GroupPick,
    cursor: Option<usize>,
    spacing: usize,
    rows: usize,
}

impl<V> GroupMultiSelect<V> {
    /// Adds a group of options under the heading `label`, `(value, label)` or
    /// `(value, label, hint)` as well as [`Choice`]s, which may be
    /// [`disabled`](Choice::disabled). A group without options is dropped.
    #[must_use]
    pub fn group<C: Into<Choice<V>>>(
        self,
        label: impl Into<String>,
        choices: impl IntoIterator<Item = C>,
    ) -> Self {
        let (values, entries) = Choice::split(choices.into_iter().map(Into::into));
        if values.is_empty() {
            return self;
        }
        let group = self.places.len();
        let count = values.len();
        let header = Entry::open(label.into(), None);
        let places =
            iter::once(Place::Header(group)).chain((1..=count).map(|number| {
                Place::Item {
                    group,
                    last: number == count,
                }
            }));
        Self {
            values: self
                .values
                .into_iter()
                .chain(iter::once(None))
                .chain(values.into_iter().map(Some))
                .collect(),
            entries: self
                .entries
                .into_iter()
                .chain(iter::once(header))
                .chain(entries)
                .collect(),
            places: self.places.into_iter().chain(places).collect(),
            ..self
        }
    }

    /// Whether a group's row can be picked; it can by default.
    #[must_use]
    pub fn selectable_groups(self, group_pick: GroupPick) -> Self {
        Self { group_pick, ..self }
    }

    /// The options that start picked, counted from 0 over all groups in order;
    /// others are ignored, and so are disabled ones.
    #[must_use]
    pub fn picked(self, indices: impl IntoIterator<Item = usize>) -> Self {
        Self {
            picked: Some(indices.into_iter().collect()),
            ..self
        }
    }

    /// Enter with nothing picked answers with an empty list instead of being
    /// refused.
    #[must_use]
    pub fn optional(self) -> Self {
        Self {
            empty: EmptyPick::Allowed,
            ..self
        }
    }

    /// The option the cursor starts on, counted like [`picked`](Self::picked);
    /// by default the first group's row, or the first option when groups are
    /// [`ItemsOnly`](GroupPick::ItemsOnly). A disabled option makes it the next
    /// one that is not.
    #[must_use]
    pub fn cursor_at(self, index: usize) -> Self {
        Self {
            cursor: Some(index),
            ..self
        }
    }

    /// How many empty rows come before each group.
    #[must_use]
    pub fn group_spacing(self, rows: usize) -> Self {
        Self {
            spacing: rows,
            ..self
        }
    }

    /// How many rows show at once, group rows included.
    #[must_use]
    pub fn max_rows(self, rows: NonZeroUsize) -> Self {
        Self {
            rows: rows.get(),
            ..self
        }
    }

    fn items(&self) -> Vec<usize> {
        (0..self.places.len())
            .filter(|at| self.is_item(*at))
            .collect()
    }

    fn is_item(&self, at: usize) -> bool {
        matches!(self.places.get(at), Some(Place::Item { .. }))
    }

    fn pickable(&self, at: usize) -> bool {
        self.is_item(at) && self.entries.get(at).is_some_and(Entry::is_open)
    }

    fn opening(&self) -> Option<Picks> {
        let items = self.items();
        let wanted = self.cursor.unwrap_or(0).min(items.len().checked_sub(1)?);
        let first = items
            .iter()
            .copied()
            .cycle()
            .skip(wanted)
            .take(items.len())
            .find(|at| self.pickable(*at))?;
        let cursor = match (self.cursor, self.group_pick) {
            (Some(_), _) | (None, GroupPick::ItemsOnly) => first,
            (None, GroupPick::Whole) => self
                .heading_of(first)
                .filter(|heading| !self.skips(*heading))
                .unwrap_or(first),
        };
        Some(Picks {
            cursor,
            on: (0..self.places.len())
                .map(|at| {
                    self.pickable(at)
                        && items.iter().position(|row| *row == at).is_some_and(|rank| {
                            self.picked
                                .as_deref()
                                .is_some_and(|picked| picked.contains(&rank))
                        })
                })
                .collect(),
        })
    }

    fn heading_of(&self, at: usize) -> Option<usize> {
        let Some(Place::Item { group, .. }) = self.places.get(at) else {
            return None;
        };
        self.places
            .iter()
            .position(|place| *place == Place::Header(*group))
    }

    fn skips(&self, at: usize) -> bool {
        let heading =
            match self.places.get(at) {
                Some(Place::Header(group)) => self.group_pick == GroupPick::ItemsOnly
                    || !self.places.iter().enumerate().any(|(row, place)| {
                        matches!(place, Place::Item { group: of, .. } if of == group)
                            && self.pickable(row)
                    }),
                _ => false,
            };
        heading || (self.is_item(at) && !self.pickable(at))
    }

    fn after(&self, cursor: usize, step: usize) -> usize {
        seek(cursor, step, self.places.len(), |at| !self.skips(at))
    }

    fn rank(&self, at: usize) -> Option<usize> {
        match self.places.get(at)? {
            Place::Header(_) => None,
            Place::Item { .. } => self.places.get(..at).map(|before| {
                before
                    .iter()
                    .filter(|place| matches!(place, Place::Item { .. }))
                    .count()
            }),
        }
    }

    fn fill(&self, group: usize, on: &[bool]) -> Fill {
        let (picked, all) = self
            .places
            .iter()
            .zip(on)
            .enumerate()
            .filter(|(at, (place, _))| {
                self.pickable(*at)
                    && matches!(place, Place::Item { group: of, .. } if *of == group)
            })
            .fold((0_usize, 0_usize), |(picked, all), (_, (_, on))| {
                (
                    picked.saturating_add(usize::from(*on)),
                    all.saturating_add(1),
                )
            });
        match picked {
            0 => Fill::Nothing,
            some if some == all => Fill::All,
            _ => Fill::Some,
        }
    }

    fn toggled(&self, state: &Picks) -> Picks {
        let at = state.cursor;
        let on = match self.places.get(at) {
            Some(&Place::Header(group)) => {
                let all = self.fill(group, &state.on) == Fill::All;
                self.places
                    .iter()
                    .zip(&state.on)
                    .enumerate()
                    .map(|(row, (place, on))| match place {
                        Place::Item { group: of, .. }
                            if *of == group && self.pickable(row) =>
                        {
                            !all
                        }
                        Place::Item { .. } | Place::Header(_) => *on,
                    })
                    .collect()
            }
            Some(Place::Item { .. }) | None => (0..)
                .zip(&state.on)
                .map(|(row, on)| *on != (row == at))
                .collect(),
        };
        Picks {
            on,
            cursor: state.cursor,
        }
    }

    fn row(
        &self,
        skin: &Skin,
        bar: &str,
        state: &Picks,
        (at, option): (usize, &Entry),
    ) -> String {
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let place = self.places.get(at).copied();
        let here = at == state.cursor;
        let under_group = !here
            && matches!(
                (place, self.places.get(state.cursor).copied()),
                (Some(Place::Item { group, .. }), Some(Place::Header(on))) if group == on
            );
        let fill = match place {
            Some(Place::Header(group)) => self.fill(group, &state.on),
            Some(Place::Item { .. }) | None => {
                Fill::from(state.on.get(at) == Some(&true))
            }
        };
        let boxed = matches!(place, Some(Place::Item { .. }))
            || self.group_pick == GroupPick::Whole;
        let check = if boxed {
            checkbox(skin, fill, (here || under_group) && option.is_open())
        } else {
            String::new()
        };
        let tree = match (place, self.group_pick) {
            (Some(Place::Item { last: true, .. }), GroupPick::Whole) => {
                format!("{} ", symbols.bar_end)
            }
            (Some(Place::Item { last: false, .. }), GroupPick::Whole) => {
                format!("{} ", symbols.bar)
            }
            (Some(Place::Item { .. }), GroupPick::ItemsOnly) => "  ".to_owned(),
            (Some(Place::Header(_)) | None, _) => String::new(),
        };
        let tree = if tree.is_empty() {
            tree
        } else {
            skin.paint(theme.rail, &tree)
        };
        let text = match (option.is_open(), here) {
            (false, _) => disabled_text(skin, option),
            (true, true) => format!(
                "{}{}",
                option.label,
                option.hinted(|hint| skin.paint(theme.hint, hint))
            ),
            (true, false) => skin.paint(theme.unselected, &option.label),
        };
        let lead = match (skin.color(), here) {
            (ColorChoice::Plain, true) => format!("{} ", symbols.cursor),
            (ColorChoice::Plain | ColorChoice::Colored, _) => "  ".to_owned(),
        };
        let gap: String = match place {
            Some(Place::Header(_)) => {
                iter::repeat_n(format!("{bar}\n"), self.spacing).collect()
            }
            Some(Place::Item { .. }) | None => String::new(),
        };
        format!("{gap}{bar}{lead}{tree}{check} {text}")
    }
}

impl<V> Prompt for GroupMultiSelect<V> {
    type Output = Vec<V>;
    type State = Picks;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Picks> {
        self.opening().ok_or(Error::NoChoices)
    }

    fn update(&self, state: Picks, key: Key) -> Next<Picks> {
        let moved = |from: Picks, step: usize| Picks {
            cursor: self.after(from.cursor, step),
            ..from
        };
        match key {
            Key::Up => Next::Editing(moved(state, self.places.len().saturating_sub(1))),
            Key::Down | Key::Tab => Next::Editing(moved(state, 1)),
            Key::Char(' ') => Next::Editing(self.toggled(&state)),
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
                let rows = scrolled(
                    skin,
                    &bar,
                    &self.entries,
                    (state.cursor, self.rows),
                    |at, option| self.row(skin, &bar, state, (at, option)),
                );
                frame.extend(
                    rows.iter()
                        .flat_map(|row| row.split('\n').map(str::to_owned))
                        .chain(draw::instructions(
                            skin,
                            &frame.messages().keys_group_multiselect,
                            frame.width(),
                        ))
                        .collect::<Vec<_>>(),
                );
            }
            Phase::Submitted => frame.row(
                labelled(&self.entries, &state.on)
                    .unwrap_or_else(|| frame.messages().none.to_string()),
            ),
            Phase::Cancelled => frame.extend(labelled(&self.entries, &state.on)),
        }
    }

    fn finish(self, state: Picks) -> Result<Vec<V>> {
        Ok(self
            .values
            .into_iter()
            .zip(state.on)
            .filter_map(|(value, on)| on.then_some(value).flatten())
            .collect())
    }

    fn printable(&self) -> Printable {
        Printable::Bound
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        let marked = self.opening().map(|picks| picks.on).unwrap_or_default();
        let rows = self.entries.iter().zip(&self.places).zip(marked).scan(
            0_usize,
            |number, ((option, place), on)| {
                Some(match place {
                    Place::Header(_) => format!("  {}", option.label),
                    Place::Item { .. } => {
                        *number = number.saturating_add(1);
                        format!(
                            "    {number}) {} {}{}",
                            option.tick(on),
                            option.label,
                            option.hinted(str::to_owned)
                        )
                    }
                })
            },
        );
        rows.chain([messages.listing_numbers.to_string()]).collect()
    }

    fn by_default(&self) -> Option<Picks> {
        self.picked.as_ref().and_then(|_| self.opening())
    }

    /// Numbers name the options to pick; none keeps the marked; else cancels.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        let (Some(named), Some(start)) =
            (named(line, self.items().len()), self.opening())
        else {
            return vec![Key::Cancel];
        };
        if named.is_empty() {
            return vec![Key::Enter];
        }
        let rests = (0..self.places.len()).filter(|at| !self.skips(*at)).count();
        walk(
            &start,
            &named,
            rests,
            (|at| self.rank(at), |cursor| self.after(cursor, 1)),
        )
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        let items = self.items();
        let named = named(line, items.len())?;
        refused_disabled(
            &named,
            |rank| items.get(rank).is_some_and(|at| self.pickable(*at)),
            messages,
        )
    }
}

impl<V> Peek for GroupMultiSelect<V> {
    type Peeked<'a>
        = Vec<&'a V>
    where
        Self: 'a;

    fn peek<'a>(&'a self, state: &'a Picks) -> Option<Vec<&'a V>> {
        Some(
            self.values
                .iter()
                .zip(&state.on)
                .filter_map(|(value, on)| on.then_some(value.as_ref()).flatten())
                .collect(),
        )
    }
}

impl<V: PartialEq> Prefill for GroupMultiSelect<V> {
    fn shown_for(&self, answer: &Vec<V>, messages: &Messages) -> Option<String> {
        let values: Vec<Option<&V>> = (0..)
            .zip(&self.values)
            .map(|(at, value)| value.as_ref().filter(|_| self.pickable(at)))
            .collect();
        picked_labels(&self.entries, &values, answer, V::eq, messages)
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use super::{GroupMultiSelect, GroupPick, group_multiselect};
    use crate::{
        core::{
            ask::{Frame, Key, Next, Phase, Prefill, Prompt},
            error::{Error, Result},
        },
        fake::{
            Script,
            screen,
            testing::{self, bare},
        },
        prompts::choice::Choice,
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
    };

    const SPACE: Key = Key::Char(' ');
    const DOWN: Key = Key::Down;
    const ENTER: Key = Key::Enter;

    /// Not `Clone` and not `Eq`: the values must move out.
    #[derive(Debug)]
    struct Plan(&'static str);

    /// Rows: Web, A, B, Cli, C.
    fn plans(group_pick: GroupPick) -> GroupMultiSelect<Plan> {
        group_multiselect("Which?")
            .group(
                "Web",
                [
                    Choice::new(Plan("a"), "Plans A"),
                    Choice::new(Plan("b"), "Plans B").hint("second"),
                ],
            )
            .group("Cli", [Choice::new(Plan("c"), "Plans C")])
            .selectable_groups(group_pick)
            .optional()
    }

    fn picked(result: &Result<Vec<Plan>>) -> String {
        testing::outcome(result, |plans| {
            format!("[{}]", plans.iter().map(|plan| plan.0).collect::<String>())
        })
    }

    #[rstest]
    #[case::a_group_picks_all_its_options(GroupPick::Whole, TerminalKind::Live, vec![SPACE, ENTER], &[], "[ab]")]
    #[case::a_group_twice_drops_them(GroupPick::Whole, TerminalKind::Live, vec![SPACE, SPACE, DOWN, DOWN, DOWN, DOWN, SPACE, ENTER], &[], "[c]")]
    #[case::an_option_toggled_twice_is_dropped(GroupPick::Whole, TerminalKind::Live, vec![DOWN, SPACE, DOWN, SPACE, SPACE, ENTER], &[], "[a]")]
    #[case::a_group_with_one_picked_picks_the_rest(GroupPick::Whole, TerminalKind::Live, vec![DOWN, SPACE, Key::Up, SPACE, ENTER], &[], "[ab]")]
    #[case::option_order_not_toggle_order(GroupPick::Whole, TerminalKind::Live, vec![Key::Up, SPACE, DOWN, SPACE, ENTER], &[], "[abc]")]
    #[case::items_only_skips_the_headings(GroupPick::ItemsOnly, TerminalKind::Live, vec![SPACE, DOWN, DOWN, SPACE, ENTER], &[], "[ac]")]
    #[case::items_only_wraps_over_the_heading(GroupPick::ItemsOnly, TerminalKind::Live, vec![Key::Up, SPACE, ENTER], &[], "[c]")]
    #[case::items_only_up_skips_the_heading(GroupPick::ItemsOnly, TerminalKind::Live, vec![DOWN, DOWN, Key::Up, SPACE, ENTER], &[], "[b]")]
    #[case::a_picks_every_option(GroupPick::Whole, TerminalKind::Live, vec![Key::Char('a'), ENTER], &[], "[abc]")]
    #[case::a_twice_picks_none(GroupPick::Whole, TerminalKind::Live, vec![Key::Char('a'), Key::Char('a'), ENTER], &[], "[]")]
    #[case::a_picks_every_option_items_only(GroupPick::ItemsOnly, TerminalKind::Live, vec![Key::Char('a'), ENTER], &[], "[abc]")]
    #[case::i_inverts_the_options(GroupPick::Whole, TerminalKind::Live, vec![SPACE, Key::Char('i'), ENTER], &[], "[c]")]
    #[case::i_twice_restores(GroupPick::Whole, TerminalKind::Live, vec![DOWN, SPACE, Key::Char('i'), Key::Char('i'), ENTER], &[], "[a]")]
    #[case::ctrl_c(GroupPick::Whole, TerminalKind::Live, vec![Key::Interrupt], &[], "interrupted")]
    #[case::escape(GroupPick::Whole, TerminalKind::Live, vec![Key::Cancel], &[], "cancelled")]
    #[case::line_numbers(GroupPick::Whole, TerminalKind::Dumb, vec![], &["1 3"], "[ac]")]
    #[case::line_numbers_items_only(GroupPick::ItemsOnly, TerminalKind::Dumb, vec![], &["2,3"], "[bc]")]
    #[case::line_keeps_the_marked(GroupPick::Whole, TerminalKind::Dumb, vec![], &[""], "[]")]
    #[case::line_out_of_range(GroupPick::Whole, TerminalKind::Dumb, vec![], &["4"], "cancelled")]
    #[case::line_end_of_input(GroupPick::Whole, TerminalKind::Dumb, vec![], &[], "cancelled")]
    #[case::piped(GroupPick::Whole, TerminalKind::Redirected, vec![], &["1"], "no input")]
    fn a_group_multiselect_answers_with_the_values_in_option_order(
        #[case] group_pick: GroupPick,
        #[case] kind: TerminalKind,
        #[case] strokes: Vec<Key>,
        #[case] lines: &'static [&'static str],
        #[case] expected: &str,
    ) {
        let ui = bare(Script::new(kind, strokes, lines.iter().copied()));
        let result = ui.ask(plans(group_pick));
        assert_eq!(picked(&result), expected, "{result:?}");
    }

    #[test]
    fn a_and_i_leave_the_headings_alone() -> Result<()> {
        let ui = bare(Script::new(
            TerminalKind::Live,
            [Key::Char('a'), Key::Char('i'), Key::Char('i'), ENTER],
            [],
        ));
        ui.ask(plans(GroupPick::Whole))?;
        let written = ui.terminal().written();
        assert!(
            written.contains("◇  Which?\n│  Plans A, Plans B, Plans C\n"),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn no_options_make_a_typed_error_and_no_question() {
        let ui = bare(Script::new(TerminalKind::Live, [ENTER], []));
        let result = ui
            .ask(group_multiselect("Which?").group::<Plan, Choice<Plan>>("Empty", []));
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    #[test]
    fn an_empty_group_is_dropped() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [SPACE, ENTER], []));
        let values = ui.ask(
            group_multiselect("Which?")
                .group::<i32, Choice<i32>>("Empty", [])
                .group("Some", [(1, "One")]),
        )?;
        assert_eq!(values, [1]);
        let written = ui.terminal().written();
        assert!(written.contains("│› ◻ Some\n│  └ ◻ One\n"), "{written:?}");
        assert!(!written.contains("Empty"), "{written:?}");
        Ok(())
    }

    #[test]
    fn an_empty_pick_is_refused_by_default_and_waits_for_a_pick() -> Result<()> {
        let strokes = [ENTER, DOWN, SPACE, ENTER];
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        let values =
            ui.ask(group_multiselect("Which?").group("Web", [(1, "One"), (2, "Two")]))?;
        assert_eq!(values, [1]);
        let written = ui.terminal().written();
        assert_eq!(
            written
                .matches("Please select at least one option.")
                .count(),
            1,
            "{written:?}"
        );
        assert!(
            written.contains("└  Please select at least one option."),
            "{written:?}"
        );
        assert!(written.contains("◇  Which?\n│  One\n│\n"), "{written:?}");
        Ok(())
    }

    #[test]
    fn optional_answers_nothing_and_shows_none() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [ENTER], []));
        let values = ui.ask(
            group_multiselect("Which?")
                .group("Web", [(1, "One")])
                .optional(),
        )?;
        assert_eq!(values, []);
        assert!(ui.terminal().written().contains("◇  Which?\n│  none\n│\n"));
        Ok(())
    }

    #[test]
    fn picked_and_cursor_at_count_the_options_not_the_headings() -> Result<()> {
        let values = group_multiselect("Which?")
            .group("Web", [(1, "One"), (2, "Two")])
            .group("Cli", [(3, "Three")])
            .picked([0])
            .cursor_at(2)
            .simulate([SPACE, ENTER])?;
        assert_eq!(values, [1, 3]);
        Ok(())
    }

    #[test]
    fn the_rows_hang_on_a_tree_and_the_hint_follows_the_cursor() -> Result<()> {
        let strokes = [DOWN, DOWN, SPACE, ENTER];
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        ui.ask(
            group_multiselect("Which?")
                .group(
                    "Web",
                    [
                        Choice::new(1, "Plans A"),
                        Choice::new(2, "Plans B").hint("second"),
                    ],
                )
                .group("Cli", [(3, "Plans C")]),
        )?;
        let written = ui.terminal().written();
        assert!(
            written.contains(
                "◆  Which?\n│› ◻ Web\n│  │ ◻ Plans A\n│  └ ◻ Plans B\n│  ◻ Cli\n│  └ ◻ Plans C\n│  ↑/↓ to navigate • Space: select • a: all • Enter: confirm\n└\n"
            ),
            "{written:?}"
        );
        assert!(written.contains("│› └ ◼ Plans B (second)"), "{written:?}");
        assert!(!written.contains("│  └ ◻ Plans B (second)"), "{written:?}");
        Ok(())
    }

    #[test]
    fn items_only_has_no_boxes_on_the_headings() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [SPACE, ENTER], []));
        ui.ask(
            group_multiselect("Which?")
                .group("Web", [(1, "One"), (2, "Two")])
                .selectable_groups(GroupPick::ItemsOnly),
        )?;
        let written = ui.terminal().written();
        assert!(
            written.contains("◆  Which?\n│   Web\n│›   ◻ One\n│    ◻ Two\n"),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn group_spacing_puts_empty_rail_rows_before_each_group() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [SPACE, ENTER], []));
        ui.ask(
            group_multiselect("Which?")
                .group("Web", [(1, "One")])
                .group("Cli", [(2, "Two")])
                .group_spacing(1),
        )?;
        let written = ui.terminal().written();
        assert!(
            written.contains(
                "◆  Which?\n│\n│› ◻ Web\n│  └ ◻ One\n│\n│  ◻ Cli\n│  └ ◻ Two\n"
            ),
            "{written:?}"
        );
        assert!(
            written.contains(
                "\x1b[7A\r\x1b[2K│› ◼ Web\x1b[1B\r\x1b[2K│  └ ◼ One\x1b[6B\r"
            ),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn a_line_group_multiselect_lists_the_groups_with_marks() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2 3"]));
        let values = ui.ask(
            group_multiselect("Which?")
                .group("Web", [(1, "One"), (2, "Two")])
                .group("Cli", [(3, "Three")])
                .picked([0]),
        )?;
        assert_eq!(values, [2, 3]);
        assert_eq!(
            ui.terminal().written(),
            "◆  Which?\n  Web\n    1) [x] One\n    2) [ ] Two\n  Cli\n    3) [ ] Three\n  numbers to pick, e.g. 1 3; Enter keeps the marked\n◇  Which?\n│  Two, Three\n│\n"
        );
        Ok(())
    }

    #[rstest]
    #[case::two_ticked(vec![DOWN, SPACE, DOWN, DOWN, DOWN, SPACE], "■  Pick\n│  A, C\n│")]
    #[case::none_ticked(vec![], "■  Pick\n│")]
    fn cancelled_group_multiselect_lists_the_ticked_options(
        #[case] keys: Vec<Key>,
        #[case] tail: &str,
    ) {
        let strokes = keys.into_iter().chain([Key::Cancel]);
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        let result = ui.ask(
            group_multiselect("Pick")
                .group("Web", [(1, "A"), (2, "B")])
                .group("Cli", [(3, "C")]),
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with(tail), "{shown}");
    }

    fn rows<V>(
        prompt: &GroupMultiSelect<V>,
        keys: &[Key],
        color: ColorChoice,
    ) -> Vec<String> {
        testing::rendered(prompt, keys.iter().copied(), Phase::Active, color, 80)
    }

    #[rstest]
    #[case::none(&[], &["│› ◻ Web", "│  │ ◻ One", "│  └ ◻ Two"])]
    #[case::some(&[DOWN, SPACE], &["│  ◪ Web", "│› │ ◼ One", "│  └ ◻ Two"])]
    #[case::all(&[DOWN, SPACE, DOWN, SPACE], &["│  ◼ Web", "│  │ ◼ One", "│› └ ◼ Two"])]
    #[case::dropped_again(&[SPACE, SPACE], &["│› ◻ Web", "│  │ ◻ One", "│  └ ◻ Two"])]
    fn a_group_row_shows_how_much_of_it_is_picked(
        #[case] keys: &[Key],
        #[case] expected: &[&str],
    ) {
        let prompt = group_multiselect("Pick").group("Web", [(1, "One"), (2, "Two")]);
        let shown = rows(&prompt, keys, ColorChoice::Plain);
        assert_eq!(
            shown.iter().take(3).map(String::as_str).collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn the_cursor_mark_leaves_a_space_before_the_tree() {
        let prompt = group_multiselect("Pick").group("Web", [(1, "One"), (2, "Two")]);
        let plain = rows(&prompt, &[DOWN], ColorChoice::Plain);
        let colored = rows(&prompt, &[DOWN], ColorChoice::Colored);
        assert_eq!(plain.get(1).map(String::as_str), Some("│› │ ◻ One"));
        assert!(
            colored
                .get(1)
                .is_some_and(|row| row.contains("\x1b[2m│ \x1b[0m")),
            "{colored:?}"
        );
    }

    #[test]
    fn colored_rows_follow_the_cursor_group() {
        let prompt = group_multiselect("Pick")
            .group("Web", [(1, "One"), (2, "Two")])
            .picked([1]);
        let bar = "\x1b[36m│\x1b[0m";
        assert_eq!(
            rows(&prompt, &[], ColorChoice::Colored).get(..3),
            Some(
                [
                    format!("{bar}  \x1b[32m◪\x1b[0m Web"),
                    format!(
                        "{bar}  \x1b[2m│ \x1b[0m\x1b[36m◻\x1b[0m \x1b[2mOne\x1b[0m"
                    ),
                    format!(
                        "{bar}  \x1b[2m└ \x1b[0m\x1b[32m◼\x1b[0m \x1b[2mTwo\x1b[0m"
                    ),
                ]
                .as_slice()
            )
        );
        assert_eq!(
            rows(&prompt, &[DOWN], ColorChoice::Colored).get(..3),
            Some(
                [
                    format!("{bar}  \x1b[32m◪\x1b[0m \x1b[2mWeb\x1b[0m"),
                    format!("{bar}  \x1b[2m│ \x1b[0m\x1b[36m◻\x1b[0m One"),
                    format!(
                        "{bar}  \x1b[2m└ \x1b[0m\x1b[32m◼\x1b[0m \x1b[2mTwo\x1b[0m"
                    ),
                ]
                .as_slice()
            )
        );
    }

    /// Rows: Web, A, B off, Cli, C, D off, Off, E off.
    fn gaps(group_pick: GroupPick) -> GroupMultiSelect<&'static str> {
        group_multiselect("Which?")
            .group(
                "Web",
                [
                    Choice::new("a", "Alpha"),
                    Choice::disabled("b", "Beta").hint("soon"),
                ],
            )
            .group(
                "Cli",
                [Choice::new("c", "Gamma"), Choice::disabled("d", "Delta")],
            )
            .group("Off", [Choice::disabled("e", "Epsilon")])
            .selectable_groups(group_pick)
            .optional()
    }

    #[rstest]
    #[case::down_skips_headings_and_disabled(vec![DOWN, SPACE, ENTER], "c")]
    #[case::down_wraps(vec![DOWN, DOWN, SPACE, ENTER], "a")]
    #[case::up_wraps(vec![Key::Up, SPACE, ENTER], "c")]
    fn the_cursor_skips_disabled_options_and_wraps(
        #[case] keys: Vec<Key>,
        #[case] expected: &str,
    ) -> Result<()> {
        let picked = gaps(GroupPick::ItemsOnly).simulate(keys)?;
        assert_eq!(picked, [expected]);
        Ok(())
    }

    #[test]
    fn with_group_rows_the_cursor_rests_on_them_and_still_skips_disabled() -> Result<()>
    {
        // Web, Alpha, Cli, Gamma, round again; Off has nothing to rest on.
        let at = |downs: usize| -> Result<Vec<&'static str>> {
            gaps(GroupPick::Whole)
                .simulate(std::iter::repeat_n(DOWN, downs).chain([SPACE, ENTER]))
        };
        assert_eq!(at(1)?, ["a"]);
        assert_eq!(at(2)?, ["c"], "Cli picks Gamma only");
        assert_eq!(at(3)?, ["c"]);
        assert_eq!(at(4)?, ["a"], "round to Web, which picks Alpha only");
        Ok(())
    }

    #[test]
    fn a_group_of_disabled_options_is_no_resting_place_for_the_cursor() -> Result<()> {
        let dead = || {
            group_multiselect("Which?")
                .group("Pro", [Choice::disabled(1, "A"), Choice::disabled(2, "B")])
                .group("Free", [(3, "C")])
                .selectable_groups(GroupPick::Whole)
        };
        // Opens on Free, not on Pro: Space picks C.
        assert_eq!(dead().simulate([SPACE, ENTER])?, [3]);
        assert_eq!(dead().simulate([Key::Up, SPACE, ENTER])?, [3]);
        Ok(())
    }

    #[test]
    fn a_group_row_is_full_when_its_open_options_are_picked() {
        let prompt = gaps(GroupPick::Whole);
        let next = prompt.start().ok().map(|state| prompt.update(state, SPACE));
        let mut frame = Frame::new(ColorChoice::Plain, Some(80));
        if let Some(Next::Editing(state)) = next {
            prompt.view(&state, Phase::Active, &mut frame);
        }
        let first = frame.rows().first().cloned().unwrap_or_default();
        assert!(first.contains("◼ Web"), "{first}");
    }

    #[rstest]
    #[case::a_picks_the_open_ones(Key::Char('a'))]
    #[case::i_inverts_the_open_ones(Key::Char('i'))]
    fn a_and_i_never_pick_a_disabled_option(#[case] key: Key) -> Result<()> {
        for group_pick in [GroupPick::Whole, GroupPick::ItemsOnly] {
            assert_eq!(gaps(group_pick).simulate([key, ENTER])?, ["a", "c"]);
        }
        Ok(())
    }

    #[test]
    fn picked_and_cursor_at_a_disabled_option_settle_on_open_ones() -> Result<()> {
        let with = |cursor: usize| {
            gaps(GroupPick::ItemsOnly)
                .cursor_at(cursor)
                .simulate([SPACE, ENTER])
        };
        assert_eq!(with(1)?, ["c"]);
        assert_eq!(with(3)?, ["a"], "past Delta and Epsilon, round to Alpha");
        assert_eq!(with(9)?, ["a"]);
        assert_eq!(
            gaps(GroupPick::ItemsOnly)
                .picked([0, 1, 3, 4])
                .simulate([ENTER])?,
            ["a"]
        );
        Ok(())
    }

    #[test]
    fn only_disabled_options_are_no_choices() {
        let result = group_multiselect("Which?")
            .group("G", [Choice::disabled(1, "A")])
            .simulate([ENTER]);
        assert!(matches!(result, Err(Error::NoChoices)), "{result:?}");
    }

    #[test]
    fn a_typed_line_naming_a_disabled_option_is_refused_with_a_text() {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        for line in ["2", "1 4", "5"] {
            let result = ui.ask(gaps(GroupPick::Whole).given_line(Some(line)));
            assert!(
                matches!(&result, Err(Error::Refused { reason, .. })
                    if *reason == Messages::english().option_disabled),
                "{line}: {result:?}"
            );
        }
    }

    #[rstest]
    #[case::whole(GroupPick::Whole)]
    #[case::items_only(GroupPick::ItemsOnly)]
    fn a_keyless_terminal_marks_disabled_options_and_walks_past_them(
        #[case] group_pick: GroupPick,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2", "1 3"]));
        assert_eq!(ui.ask(gaps(group_pick))?, ["a", "c"]);
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.contains(
                "  Web\n    1) [ ] Alpha\n    2) [-] Beta (soon)\n  Cli\n    3) [ ] Gamma\n    4) [-] Delta\n  Off\n    5) [-] Epsilon"
            ),
            "{shown}"
        );
        assert!(shown.contains("That option is disabled."), "{shown}");
        Ok(())
    }

    #[test]
    fn a_given_answer_with_a_disabled_value_is_refused() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let refused = ui.ask(gaps(GroupPick::Whole).given(Some(vec!["a", "b"])));
        assert!(
            matches!(&refused, Err(Error::Refused { question, .. }) if question == "Which?"),
            "{refused:?}"
        );
        assert_eq!(
            ui.ask(gaps(GroupPick::Whole).given(Some(vec!["a", "c"])))?,
            ["a", "c"]
        );
        Ok(())
    }

    #[test]
    fn a_disabled_option_looks_dim_and_struck_and_shows_its_hint() {
        let prompt = gaps(GroupPick::Whole);
        let rows = |color| {
            let mut frame = Frame::new(color, Some(80));
            if let Ok(state) = prompt.start() {
                prompt.view(&state, Phase::Active, &mut frame);
            }
            frame.rows().to_vec()
        };
        assert_eq!(
            rows(ColorChoice::Plain),
            [
                "│› ◻ Web",
                "│  │ ◻ Alpha",
                "│  └ ◻ Beta (soon)",
                "│  ◻ Cli",
                "│  │ ◻ Gamma",
                "│  └ ◻ Delta",
                "│  ◻ Off",
                "│  └ ◻ Epsilon",
                "│  ↑/↓ to navigate • Space: select • a: all • Enter: confirm"
            ]
        );
        let beta = rows(ColorChoice::Colored)
            .get(2)
            .cloned()
            .unwrap_or_default();
        assert!(beta.contains("\x1b[2m\x1b[9mBeta\x1b[0m"), "{beta:?}");
    }
}
