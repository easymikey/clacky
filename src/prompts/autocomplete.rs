//! Options narrowed down by typing: one value, or any number of them.

use std::{fmt, num::NonZeroUsize};

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, WhenSingle},
        error::{Error, Result},
    },
    prompts::{
        caret,
        choice::{
            CHOICE_ROWS,
            Choice,
            EmptyPick,
            Entry,
            Options,
            checkbox_row,
            named,
            radio_option,
            rail,
            refused_disabled,
            scrolled,
            seek,
        },
    },
    render::{
        color::measure_text_width,
        draw,
        messages::{Messages, Refusal},
        style::Skin,
    },
    values::line::{Cancellation, CharFilter, TypedAnswer},
};

const NAMED_AT_MOST: usize = 3;

/// The options of an autocomplete and the rule that narrows them down.
///
/// ```
/// # use clacky::{Choice, Source};
/// let by_first_letter =
///     Source::from(vec![Choice::new(1, "Apple"), Choice::new(2, "Banana")]).matching(
///         |typed| {
///             [0, 1]
///                 .into_iter()
///                 .filter(|at| (*at == 0) == typed.starts_with('a'))
///                 .collect()
///         },
///     );
/// # drop(by_first_letter);
/// ```
#[derive(Debug)]
pub struct Source<'f, V> {
    choices: Vec<Choice<V>>,
    matcher: Matcher<'f>,
}

type Find<'f> = Box<dyn Fn(&str) -> Vec<usize> + 'f>;

enum Matcher<'f> {
    Substring,
    Custom(Find<'f>),
}

impl fmt::Debug for Matcher<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Substring => "Substring",
            Self::Custom(_) => "Custom",
        })
    }
}

fn contains_folded(text: &str, needle: &str) -> bool {
    if text.is_ascii() && needle.is_ascii() {
        needle.is_empty()
            || text
                .as_bytes()
                .windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    } else {
        text.to_lowercase().contains(needle)
    }
}

impl Matcher<'_> {
    fn places(&self, entries: &[Entry], typed: &str) -> Vec<usize> {
        self.among(entries, typed, None)
    }

    /// A substring that still holds the old text can only drop places, so look
    /// no further than `earlier`.
    fn narrowed(
        &self,
        entries: &[Entry],
        typed: &str,
        (before, earlier): (&str, &[usize]),
    ) -> Vec<usize> {
        let holds_before = typed.to_lowercase().contains(&before.to_lowercase());
        self.among(entries, typed, holds_before.then_some(earlier))
    }

    fn among(
        &self,
        entries: &[Entry],
        typed: &str,
        earlier: Option<&[usize]>,
    ) -> Vec<usize> {
        match self {
            Self::Substring => {
                let needle = typed.to_lowercase();
                let holds = |text: &str| contains_folded(text, &needle);
                let keeps = |at: &usize| {
                    entries.get(*at).is_some_and(|entry| {
                        holds(&entry.label) || entry.hint.as_deref().is_some_and(holds)
                    })
                };
                earlier.map_or_else(
                    || (0..entries.len()).filter(keeps).collect(),
                    |earlier| earlier.iter().copied().filter(keeps).collect(),
                )
            }
            Self::Custom(find) => {
                let mut seen = vec![false; entries.len()];
                find(typed)
                    .into_iter()
                    .filter(|at| {
                        seen.get_mut(*at)
                            .is_some_and(|shown| !std::mem::replace(shown, true))
                    })
                    .collect()
            }
        }
    }
}

impl<V> From<Vec<Choice<V>>> for Source<'_, V> {
    fn from(choices: Vec<Choice<V>>) -> Self {
        Self {
            choices,
            matcher: Matcher::Substring,
        }
    }
}

impl<'f, V> Source<'f, V> {
    /// Narrows by `find` instead: it gets the typed text, also empty, and
    /// answers the places of the options to show, counted from 0, in the order
    /// to show. Places past the last option and repeated places are ignored.
    #[must_use]
    pub fn matching(self, find: impl Fn(&str) -> Vec<usize> + 'f) -> Self {
        Self {
            matcher: Matcher::Custom(Box::new(find)),
            ..self
        }
    }
}

#[derive(Debug)]
struct Many {
    picked: Option<Vec<usize>>,
    empty: EmptyPick,
}

impl Many {
    fn start(&self) -> &[usize] {
        self.picked.as_deref().unwrap_or_default()
    }
}

#[derive(Debug)]
struct Search<'f, V> {
    options: Options<V>,
    matcher: Matcher<'f>,
    placeholder: String,
    rows: usize,
}

impl<'f, V> Search<'f, V> {
    fn new() -> Self {
        Self {
            options: Options::new(),
            matcher: Matcher::Substring,
            placeholder: String::new(),
            rows: CHOICE_ROWS,
        }
    }

    fn items<C: Into<Choice<V>>>(self, choices: impl IntoIterator<Item = C>) -> Self {
        Self {
            options: self.options.extend(choices),
            ..self
        }
    }

    fn hint(self, hint: impl Into<String>) -> Self {
        Self {
            options: self.options.hint(hint),
            ..self
        }
    }

    fn source(self, source: Source<'f, V>) -> Self {
        Self {
            options: Options::new().extend(source.choices),
            matcher: source.matcher,
            ..self
        }
    }

    fn placeholder(self, text: impl Into<String>) -> Self {
        Self {
            placeholder: text.into(),
            ..self
        }
    }

    fn window(self, rows: NonZeroUsize) -> Self {
        Self {
            rows: rows.get(),
            ..self
        }
    }
}

/// Starts a question with one answer out of options that narrow down as the
/// person types. Enter with no option left does nothing.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let tool: &str = ui.ask(
///     autocomplete("Which tool?")
///         .item("fmt", "Formatter")
///         .item("lint", "Linter")
///         .hint("slow on big trees")
///         .placeholder("fmt"),
/// )?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn autocomplete(message: impl Into<String>) -> EmptyAutocomplete {
    EmptyAutocomplete {
        message: message.into(),
    }
}

/// An [`autocomplete`] without options.
#[derive(Debug)]
pub struct EmptyAutocomplete {
    message: String,
}

impl EmptyAutocomplete {
    fn with<V>(self, search: Search<'_, V>) -> Autocomplete<'_, V> {
        Autocomplete {
            message: self.message,
            search,
            single: WhenSingle::default(),
        }
    }

    /// The first option.
    pub fn item<'f, V>(
        self,
        value: V,
        label: impl Into<String>,
    ) -> Autocomplete<'f, V> {
        self.items([Choice::new(value, label)])
    }

    /// The first option, disabled: shown, but never under the cursor.
    pub fn disabled<'f, V>(
        self,
        value: V,
        label: impl Into<String>,
    ) -> Autocomplete<'f, V> {
        self.items([Choice::disabled(value, label)])
    }

    /// The first options; they may be none, which fails the question with
    /// [`Error::NoChoices`] when it runs.
    pub fn items<'f, V, C: Into<Choice<V>>>(
        self,
        choices: impl IntoIterator<Item = C>,
    ) -> Autocomplete<'f, V> {
        self.with(Search::new().items(choices))
    }

    /// The options and the rule that narrows them down.
    pub fn source<'f, V>(
        self,
        source: impl Into<Source<'f, V>>,
    ) -> Autocomplete<'f, V> {
        self.with(Search::new().source(source.into()))
    }
}

/// A question with one answer out of the options added so far.
#[derive(Debug)]
pub struct Autocomplete<'f, V> {
    message: String,
    search: Search<'f, V>,
    single: WhenSingle,
}

impl<'f, V> Autocomplete<'f, V> {
    fn searched(self, change: impl FnOnce(Search<'f, V>) -> Search<'f, V>) -> Self {
        Self {
            search: change(self.search),
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

    /// Adds an option.
    #[must_use]
    pub fn item(self, value: V, label: impl Into<String>) -> Self {
        self.items([Choice::new(value, label)])
    }

    /// Adds a disabled option: shown dim and struck through, skipped by the
    /// cursor, never picked by a key and refused when a typed number names it.
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
        self.searched(|search| search.items(choices))
    }

    /// Shows dim after the last option while the cursor is on it; empty is
    /// none.
    #[must_use]
    pub fn hint(self, hint: impl Into<String>) -> Self {
        self.searched(|search| search.hint(hint))
    }

    /// The options and the rule that narrows them down, replacing the options
    /// given before.
    #[must_use]
    pub fn source(self, source: impl Into<Source<'f, V>>) -> Self {
        self.searched(|search| search.source(source.into()))
    }

    /// Dim text shown while nothing is typed; Tab takes it as typed when it
    /// finds an option.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Self {
        self.searched(|search| search.placeholder(text))
    }

    /// How many options show at once.
    #[must_use]
    pub fn max_rows(self, rows: NonZeroUsize) -> Self {
        self.searched(|search| search.window(rows))
    }
}

/// Starts a question with any number of answers out of options that narrow down
/// as the person types. Enter with nothing picked is refused unless
/// [`optional`](AutocompleteMultiSelect::optional).
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let tools: Vec<&str> = ui.ask(
///     autocomplete_multiselect("Which tools?")
///         .item("fmt", "Formatter")
///         .item("lint", "Linter")
///         .picked([0])
///         .optional(),
/// )?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn autocomplete_multiselect(
    message: impl Into<String>,
) -> EmptyAutocompleteMultiSelect {
    EmptyAutocompleteMultiSelect {
        message: message.into(),
    }
}

/// An [`autocomplete_multiselect`] without options.
#[derive(Debug)]
pub struct EmptyAutocompleteMultiSelect {
    message: String,
}

impl EmptyAutocompleteMultiSelect {
    fn with<V>(self, search: Search<'_, V>) -> AutocompleteMultiSelect<'_, V> {
        AutocompleteMultiSelect {
            message: self.message,
            search,
            many: Many {
                picked: None,
                empty: EmptyPick::Refused,
            },
        }
    }

    /// The first option.
    pub fn item<'f, V>(
        self,
        value: V,
        label: impl Into<String>,
    ) -> AutocompleteMultiSelect<'f, V> {
        self.items([Choice::new(value, label)])
    }

    /// The first option, disabled: shown, but never under the cursor.
    pub fn disabled<'f, V>(
        self,
        value: V,
        label: impl Into<String>,
    ) -> AutocompleteMultiSelect<'f, V> {
        self.items([Choice::disabled(value, label)])
    }

    /// The first options; they may be none, which fails the question with
    /// [`Error::NoChoices`] when it runs.
    pub fn items<'f, V, C: Into<Choice<V>>>(
        self,
        choices: impl IntoIterator<Item = C>,
    ) -> AutocompleteMultiSelect<'f, V> {
        self.with(Search::new().items(choices))
    }

    /// The options and the rule that narrows them down.
    pub fn source<'f, V>(
        self,
        source: impl Into<Source<'f, V>>,
    ) -> AutocompleteMultiSelect<'f, V> {
        self.with(Search::new().source(source.into()))
    }
}

/// A question with any number of answers out of the options added so far.
#[derive(Debug)]
pub struct AutocompleteMultiSelect<'f, V> {
    message: String,
    search: Search<'f, V>,
    many: Many,
}

impl<'f, V> AutocompleteMultiSelect<'f, V> {
    fn searched(self, change: impl FnOnce(Search<'f, V>) -> Search<'f, V>) -> Self {
        Self {
            search: change(self.search),
            ..self
        }
    }

    /// Adds an option.
    #[must_use]
    pub fn item(self, value: V, label: impl Into<String>) -> Self {
        self.items([Choice::new(value, label)])
    }

    /// Adds a disabled option: shown dim and struck through, skipped by the
    /// cursor, never picked by a key and refused when a typed number names it.
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
        self.searched(|search| search.items(choices))
    }

    /// Shows dim after the last option while the cursor is on it; empty is
    /// none.
    #[must_use]
    pub fn hint(self, hint: impl Into<String>) -> Self {
        self.searched(|search| search.hint(hint))
    }

    /// The options and the rule that narrows them down, replacing the options
    /// given before.
    #[must_use]
    pub fn source(self, source: impl Into<Source<'f, V>>) -> Self {
        self.searched(|search| search.source(source.into()))
    }

    /// Dim text shown while nothing is typed.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Self {
        self.searched(|search| search.placeholder(text))
    }

    /// How many options show at once.
    #[must_use]
    pub fn max_rows(self, rows: NonZeroUsize) -> Self {
        self.searched(|search| search.window(rows))
    }

    /// The options that start picked, counted from 0 in the order they were
    /// added; others are ignored, and so are disabled ones.
    #[must_use]
    pub fn picked(self, indices: impl IntoIterator<Item = usize>) -> Self {
        Self {
            many: Many {
                picked: Some(indices.into_iter().collect()),
                ..self.many
            },
            ..self
        }
    }

    /// Enter with nothing picked answers an empty list instead of being
    /// refused.
    #[must_use]
    pub fn optional(self) -> Self {
        Self {
            many: Many {
                empty: EmptyPick::Allowed,
                ..self.many
            },
            ..self
        }
    }
}

/// The typed text, the options it leaves, the cursor and what is picked.
#[derive(Debug)]
pub struct Found {
    query: TypedAnswer,
    shown: Vec<usize>,
    /// Place in `shown`; none while every option shown is disabled.
    cursor: Option<usize>,
    /// For each option whether it is picked; empty for one answer.
    picked: Vec<bool>,
    /// Space picks only after an arrow; typing leaves this mode.
    navigating: bool,
}

impl Found {
    fn focused(&self) -> Option<usize> {
        self.cursor
            .and_then(|cursor| self.shown.get(cursor))
            .copied()
    }

    fn picks(&self) -> Vec<usize> {
        (0..)
            .zip(&self.picked)
            .filter_map(|(at, on)| on.then_some(at))
            .collect()
    }

    fn stepped(self, step: usize, open: impl Fn(usize) -> bool) -> Self {
        let cursor = self.cursor.map(|from| {
            seek(from, step, self.shown.len(), |place| {
                self.shown.get(place).is_some_and(|at| open(*at))
            })
        });
        Self { cursor, ..self }
    }

    fn toggled(self) -> Self {
        let Some(at) = self.focused() else {
            return self;
        };
        let mut picked = self.picked;
        if let Some(on) = picked.get_mut(at) {
            *on = !*on;
        }
        Self { picked, ..self }
    }
}

impl<V> Search<'_, V> {
    fn start(&self, picked: Vec<bool>) -> Result<Found> {
        if self.options.open_places().next().is_none() {
            return Err(Error::NoChoices);
        }
        let shown = self.matcher.places(&self.options.entries, "");
        Ok(Found {
            query: TypedAnswer::new(CharFilter::Printable),
            cursor: self.first_open(&shown),
            shown,
            picked,
            navigating: false,
        })
    }

    fn first_open(&self, shown: &[usize]) -> Option<usize> {
        shown.iter().position(|at| self.options.open(*at))
    }

    fn placeheld(&self, found: Found) -> Found {
        let takes = found.query.text().is_empty()
            && !self.placeholder.is_empty()
            && self
                .matcher
                .places(&self.options.entries, &self.placeholder)
                .iter()
                .any(|at| self.options.open(*at));
        if takes {
            let query =
                TypedAnswer::new(CharFilter::Printable).prefilled(&self.placeholder);
            self.narrowed(Found { query, ..found }, "")
        } else {
            found
        }
    }

    fn narrowed(&self, found: Found, before: &str) -> Found {
        let shown = self.matcher.narrowed(
            &self.options.entries,
            found.query.text(),
            (before, &found.shown),
        );
        Found {
            cursor: self.first_open(&shown),
            shown,
            navigating: false,
            ..found
        }
    }

    fn edited(&self, found: Found, key: Key) -> Found {
        let before = found.query.text().to_owned();
        let query = found.query.edit(key);
        let changed = query.text() != before;
        let found = Found { query, ..found };
        if changed {
            self.narrowed(found, &before)
        } else {
            found
        }
    }

    fn summary(&self, places: &[usize], messages: &Messages) -> String {
        match places {
            [] => messages.none.to_string(),
            many if many.len() > NAMED_AT_MOST => (messages.items_selected)(many.len()),
            some => some
                .iter()
                .map(|at| self.options.label(*at))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    fn open(
        &self,
        found: &Found,
        (skin, bar): (&Skin, &str),
        (keys, width): (&str, Option<usize>),
        row: impl Fn(bool, usize, &Entry) -> String,
    ) -> Vec<String> {
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let room = width.map(|width| width.saturating_sub(4).max(1));
        let options = if found.shown.is_empty() {
            vec![format!(
                "{}  {}",
                skin.paint(theme.warning, symbols.bar),
                skin.paint(theme.warning, &skin.messages().no_matches)
            )]
        } else {
            scrolled(
                skin,
                bar,
                &found.shown,
                (found.cursor.unwrap_or(0), self.rows),
                |place, at| {
                    self.options
                        .entries
                        .get(*at)
                        .map(|entry| row(found.cursor == Some(place), *at, entry))
                        .unwrap_or_default()
                },
            )
        };
        self.search_rows(found, skin, (bar, room))
            .into_iter()
            .chain(options)
            .chain(draw::instructions(skin, keys, width))
            .collect()
    }

    fn search_rows(
        &self,
        found: &Found,
        skin: &Skin,
        (bar, room): (&str, Option<usize>),
    ) -> Vec<String> {
        let count = found.shown.len();
        let filtered = !found.query.text().is_empty() && count > 0;
        let search = &*skin.messages().search;
        let suffix = if filtered {
            let many = (skin.messages().matches)(count);
            skin.paint(skin.theme().hint, &format!(" ({many})"))
        } else {
            String::new()
        };
        let room = room.map(|room| {
            room.saturating_sub(search.len() + measure_text_width(&suffix))
                .max(1)
        });
        let rows = caret::rows(
            skin,
            (found.query.text(), found.query.cursor()),
            &self.placeholder,
            room,
        );
        let last = rows.len().saturating_sub(1);
        rows.into_iter()
            .zip(0..)
            .map(|(row, number)| {
                let lead = if number == 0 {
                    search.to_owned()
                } else {
                    " ".repeat(search.len())
                };
                let tail = if number == last { suffix.as_str() } else { "" };
                format!("{bar}  {lead}{row}{tail}")
            })
            .collect()
    }

    fn numbered(&self, line: &str) -> Option<usize> {
        line.parse::<usize>()
            .ok()
            .filter(|number| (1..=self.options.entries.len()).contains(number))
    }

    fn typed_one(&self, line: &str) -> Vec<Key> {
        self.numbered(line).map_or_else(
            || line.chars().map(Key::Char).chain([Key::Enter]).collect(),
            |number| {
                let shown = self.matcher.places(&self.options.entries, "");
                shown.iter().position(|at| *at + 1 == number).map_or_else(
                    || vec![Key::Cancel],
                    |place| {
                        let before = shown
                            .iter()
                            .take(place)
                            .filter(|at| self.options.open(**at))
                            .count();
                        std::iter::repeat_n(Key::Down, before)
                            .chain([Key::Enter])
                            .collect()
                    },
                )
            },
        )
    }

    fn typed_many(&self, line: &str, start: &[usize]) -> Vec<Key> {
        match named(line, self.options.entries.len()) {
            None => vec![Key::Cancel],
            Some(named) if named.is_empty() => vec![Key::Enter],
            Some(named) => self
                .matcher
                .places(&self.options.entries, "")
                .iter()
                .filter(|at| self.options.open(**at))
                .flat_map(|at| {
                    (start.contains(at) != named.contains(&(*at + 1)))
                        .then_some(Key::Tab)
                        .into_iter()
                        .chain([Key::Down])
                })
                .chain([Key::Enter])
                .collect(),
        }
    }
}

impl<V> Prompt for Autocomplete<'_, V> {
    type Output = V;
    type State = Found;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Found> {
        self.search.start(Vec::new())
    }

    fn update(&self, found: Found, key: Key) -> Next<Found> {
        match key {
            Key::Up => {
                let back = found.shown.len().saturating_sub(1);
                Next::Editing(found.stepped(back, |at| self.search.options.open(at)))
            }
            Key::Down => {
                Next::Editing(found.stepped(1, |at| self.search.options.open(at)))
            }
            Key::Tab => Next::Editing(self.search.placeheld(found)),
            Key::Enter if found.focused().is_some() => Next::Submit(found),
            Key::Enter => Next::Editing(found),
            Key::Interrupt => Next::Quit(found, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(found, Cancellation::Cancel),
            Key::EndOfInput if found.query.text().is_empty() => {
                Next::Quit(found, Cancellation::Cancel)
            }
            Key::EndOfInput
            | Key::Char(_)
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(self.search.edited(found, key)),
        }
    }

    fn view(&self, found: &Found, phase: Phase<'_>, frame: &mut Frame) {
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let skin = frame.skin();
                let bar = rail(skin, phase);
                let rows = self.search.open(
                    found,
                    (skin, &bar),
                    (&frame.messages().keys_autocomplete, frame.width()),
                    |here, _, entry| {
                        let row = radio_option(skin, here, entry);
                        format!("{bar}  {row}")
                    },
                );
                frame.extend(rows);
            }
            Phase::Submitted => {
                frame.row(found.focused().map_or_else(String::new, |at| {
                    self.search.options.label(at).to_owned()
                }));
            }
            Phase::Cancelled => frame.row(found.query.text().to_owned()),
        }
    }

    fn finish(self, found: Found) -> Result<V> {
        let at = found.focused().ok_or(Error::NoChoices)?;
        self.search
            .options
            .values
            .into_iter()
            .nth(at)
            .ok_or(Error::NoChoices)
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        let rows = (1..)
            .zip(&self.search.options.entries)
            .map(|(number, entry)| {
                format!(
                    "  {number}) {}{}{}",
                    entry.dash(),
                    entry.label,
                    entry.hinted(str::to_owned)
                )
            });
        rows.chain([messages.listing_name.to_string()]).collect()
    }

    /// A number picks that option; other text narrows the options and takes the
    /// first.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        self.search.typed_one(line.trim())
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        let line = line.trim();
        let Some(number) = self.search.numbered(line) else {
            let shown = self
                .search
                .matcher
                .places(&self.search.options.entries, line);
            let shut = !shown.is_empty()
                && !shown.iter().any(|at| self.search.options.open(*at));
            return shut.then(|| Refusal::Said(messages.option_disabled.to_string()));
        };
        refused_disabled(&[number], |at| self.search.options.open(at), messages)
    }

    fn sole(&self) -> Option<(WhenSingle, Found)> {
        let mut open = self.search.options.open_places();
        open.next().filter(|_| open.next().is_none())?;
        self.search
            .start(Vec::new())
            .ok()
            .filter(|found| found.focused().is_some())
            .map(|found| (self.single, found))
    }
}

impl<V> Peek for Autocomplete<'_, V> {
    type Peeked<'a>
        = &'a V
    where
        Self: 'a;

    fn peek<'a>(&'a self, found: &'a Found) -> Option<&'a V> {
        found
            .focused()
            .and_then(|at| self.search.options.values.get(at))
    }
}

impl<V: PartialEq> Prefill for Autocomplete<'_, V> {
    fn shown_for(&self, answer: &V, _messages: &Messages) -> Option<String> {
        let options = &self.search.options;
        options
            .values
            .iter()
            .enumerate()
            .find(|(at, value)| options.open(*at) && *value == answer)
            .map(|(at, _)| options.label(at).to_owned())
    }
}

impl<V> Prompt for AutocompleteMultiSelect<'_, V> {
    type Output = Vec<V>;
    type State = Found;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<Found> {
        let picked = (0..self.search.options.entries.len())
            .map(|at| self.search.options.open(at) && self.many.start().contains(&at))
            .collect();
        self.search.start(picked)
    }

    fn update(&self, found: Found, key: Key) -> Next<Found> {
        match key {
            Key::Up => {
                let back = found.shown.len().saturating_sub(1);
                Next::Editing(Found {
                    navigating: true,
                    ..found.stepped(back, |at| self.search.options.open(at))
                })
            }
            Key::Down => Next::Editing(Found {
                navigating: true,
                ..found.stepped(1, |at| self.search.options.open(at))
            }),
            Key::Tab => Next::Editing(found.toggled()),
            Key::Char(' ') if found.navigating => Next::Editing(found.toggled()),
            Key::Enter
                if self.many.empty == EmptyPick::Refused
                    && found.picks().is_empty() =>
            {
                Next::Refuse(found, Refusal::NoItem)
            }
            Key::Enter => Next::Submit(found),
            Key::Interrupt => Next::Quit(found, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(found, Cancellation::Cancel),
            Key::EndOfInput if found.query.text().is_empty() => {
                Next::Quit(found, Cancellation::Cancel)
            }
            Key::EndOfInput
            | Key::Char(_)
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(self.search.edited(found, key)),
        }
    }

    fn view(&self, found: &Found, phase: Phase<'_>, frame: &mut Frame) {
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let skin = frame.skin();
                let bar = rail(skin, phase);
                let messages = frame.messages();
                let keys = if found.navigating {
                    &messages.keys_autocomplete_multiselect_navigating
                } else {
                    &messages.keys_autocomplete_multiselect
                };
                let rows = self.search.open(
                    found,
                    (skin, &bar),
                    (keys, frame.width()),
                    |here, at, entry| {
                        checkbox_row(
                            skin,
                            &bar,
                            (here, found.picked.get(at) == Some(&true)),
                            entry,
                        )
                    },
                );
                frame.extend(rows);
            }
            Phase::Submitted => {
                let named = self.search.summary(&found.picks(), frame.messages());
                frame.row(named);
            }
            Phase::Cancelled => frame.row(match found.picks().as_slice() {
                [] => String::new(),
                places => self.search.summary(places, frame.messages()),
            }),
        }
    }

    fn finish(self, found: Found) -> Result<Vec<V>> {
        let places = found.picks();
        Ok(self
            .search
            .options
            .values
            .into_iter()
            .zip(0..)
            .filter_map(|(value, at)| places.contains(&at).then_some(value))
            .collect())
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        let rows = (0..).zip(&self.search.options.entries).map(|(at, entry)| {
            format!(
                "  {}) {} {}{}",
                at + 1,
                entry.tick(self.many.start().contains(&at)),
                entry.label,
                entry.hinted(str::to_owned)
            )
        });
        rows.chain([messages.listing_numbers.to_string()]).collect()
    }

    /// Numbers name the options to pick, none keeps the marked, else cancels.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        self.search.typed_many(line, self.many.start())
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        let named = named(line, self.search.options.entries.len())?;
        refused_disabled(&named, |at| self.search.options.open(at), messages)
    }

    fn by_default(&self) -> Option<Found> {
        self.many.picked.as_ref().and_then(|_| self.start().ok())
    }
}

impl<V> Peek for AutocompleteMultiSelect<'_, V> {
    type Peeked<'a>
        = Vec<&'a V>
    where
        Self: 'a;

    fn peek<'a>(&'a self, found: &'a Found) -> Option<Vec<&'a V>> {
        Some(
            found
                .picks()
                .into_iter()
                .filter_map(|at| self.search.options.values.get(at))
                .collect(),
        )
    }
}

impl<V: PartialEq> Prefill for AutocompleteMultiSelect<'_, V> {
    fn shown_for(&self, answer: &Vec<V>, messages: &Messages) -> Option<String> {
        let options = &self.search.options;
        let known = answer.iter().all(|picked| {
            (0..)
                .zip(&options.values)
                .any(|(at, value)| options.open(at) && value == picked)
        });
        let places: Vec<usize> = (0..)
            .zip(&options.values)
            .filter_map(|(at, value)| {
                (options.open(at) && answer.contains(value)).then_some(at)
            })
            .collect();
        known.then(|| self.search.summary(&places, messages))
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::num::NonZeroUsize;

    use rstest::rstest;

    use super::{
        Autocomplete,
        AutocompleteMultiSelect,
        Source,
        autocomplete,
        autocomplete_multiselect,
    };
    use crate::{
        core::{
            ask::{Frame, Key, Peek, Phase, Prefill, Prompt},
            error::{Error, Result},
            session::Session,
            settings::Settings,
        },
        fake::{
            Script,
            screen,
            testing::{self, after, bare, keys, rendered as rendered_by},
        },
        prompts::choice::Choice,
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
    };

    /// Not `Clone` and not `Eq`: the values must move out.
    #[derive(Debug)]
    struct Fruit(&'static str);

    fn fruits() -> Vec<Choice<Fruit>> {
        vec![
            Choice::new(Fruit("apple"), "Apple").hint("red"),
            Choice::new(Fruit("banana"), "Banana"),
            Choice::new(Fruit("cherry"), "Cherry").hint("sour"),
            Choice::new(Fruit("date"), "Date"),
        ]
    }

    fn one() -> Autocomplete<'static, Fruit> {
        autocomplete("Which?").items(fruits())
    }

    fn many() -> AutocompleteMultiSelect<'static, Fruit> {
        autocomplete_multiselect("Which?")
            .items(fruits())
            .picked([3])
    }

    fn live(spec: &str) -> Session<Script> {
        bare(Script::new(TerminalKind::Live, keys(spec), []))
    }

    fn names(fruits: &[Fruit]) -> String {
        fruits
            .iter()
            .map(|fruit| fruit.0)
            .collect::<Vec<_>>()
            .join("+")
    }

    fn written_by(spec: &str) -> String {
        let form = live(spec);
        assert!(form.ask(one()).is_err());
        form.terminal().written()
    }

    fn frames_by(spec: &str) -> Vec<String> {
        let form = live(spec);
        assert!(form.ask(one()).is_err());
        form.terminal().frames()
    }

    fn outcome(result: &Result<impl ToString>) -> String {
        testing::outcome(result, ToString::to_string)
    }

    fn rendered<P: Prompt>(prompt: &P, spec: &str) -> Vec<String> {
        rendered_by(prompt, keys(spec), Phase::Active, ColorChoice::Plain, 60)
    }

    #[rstest]
    #[case::first_option("\n", "apple")]
    #[case::arrow_down("↓\n", "banana")]
    #[case::arrow_up_wraps("↑\n", "date")]
    #[case::arrow_down_wraps("↓↓↓↓\n", "apple")]
    #[case::typed("che\n", "cherry")]
    #[case::any_case("CHER\n", "cherry")]
    #[case::hint_matches("sour\n", "cherry")]
    #[case::narrowed_then_down("an↓\n", "banana")]
    #[case::cursor_back_to_first_on_typing("↓↓a\n", "apple")]
    #[case::backspace_widens("zz⌫⌫\n", "apple")]
    #[case::no_match_does_nothing_until_there_is_one("zz\n⌫⌫b\n", "banana")]
    #[case::escape("␛", "cancelled")]
    #[case::ctrl_c("☠", "interrupted")]
    #[case::ctrl_d_on_nothing_typed("⌁", "cancelled")]
    #[case::keys_run_out("↓", "no input")]
    fn an_autocomplete_answers_with_the_value(
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let result = one().simulate(keys(spec)).map(|fruit| fruit.0);
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[rstest]
    #[case::typed_text_decides("x↓\n", "apple")]
    #[case::empty_text_decides_too("\n", "date")]
    fn a_source_function_decides_what_matches(
        #[case] spec: &str,
        #[case] expected: &str,
    ) -> Result<()> {
        let found = autocomplete("Which?")
            .source(Source::from(fruits()).matching(|typed| {
                if typed.is_empty() {
                    vec![3, 1, 99]
                } else {
                    vec![2, 0]
                }
            }))
            .simulate(keys(spec))?;
        assert_eq!(found.0, expected);
        Ok(())
    }

    #[test]
    fn a_source_function_shows_a_place_once() {
        let prompt = autocomplete("Which?")
            .source(Source::from(fruits()).matching(|_| vec![1, 1, 0, 1, 0, 7]));
        let shown = rendered(&prompt, "");
        assert_eq!(
            shown,
            [
                "│  Search: ▏",
                "│  ● Banana",
                "│  ○ Apple",
                "│  ↑/↓ to select • Enter: confirm • Type: to search"
            ]
        );
        let down = prompt.simulate(keys("↓↓\n")).map(|fruit| fruit.0);
        assert_eq!(outcome(&down), "banana");
    }

    #[rstest]
    #[case::found("date", "date")]
    #[case::missing("zzz", "apple")]
    fn tab_takes_the_placeholder_when_it_finds_an_option(
        #[case] placeholder: &str,
        #[case] expected: &str,
    ) -> Result<()> {
        let found = one().placeholder(placeholder).simulate(keys("⇥\n"))?;
        assert_eq!(found.0, expected);
        Ok(())
    }

    #[rstest]
    #[case::keeps_the_start("\n", "date")]
    #[case::tab_picks("⇥\n", "apple+date")]
    #[case::tab_twice_unpicks("⇥⇥\n", "date")]
    #[case::option_order_not_pick_order("↓↓⇥↑↑⇥\n", "apple+cherry+date")]
    #[case::space_types_before_an_arrow(" \n", "date")]
    #[case::space_picks_after_an_arrow("↓ \n", "banana+date")]
    #[case::typing_ends_navigation("↓a \n", "date")]
    #[case::tab_picks_a_found_option("che⇥\n", "cherry+date")]
    #[case::picks_stay_when_the_text_changes("⇥che⌫⌫⌫\n", "apple+date")]
    #[case::escape("⇥␛", "cancelled")]
    fn an_autocomplete_multiselect_answers_in_option_order(
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let result = many().simulate(keys(spec)).map(|got| names(&got));
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[test]
    fn the_multiselect_refuses_nothing_unless_optional() {
        let optional = many().optional().simulate(keys("↓↓↓⇥\n"));
        assert_eq!(outcome(&optional.map(|got| names(&got))), "");
        let refused = many().simulate(keys("↓↓↓⇥\n"));
        assert!(
            matches!(refused, Err(Error::Refused { ref reason, .. }) if reason == "Please select at least one item."),
            "{refused:?}"
        );
        let refusing = live("↓↓↓⇥\n⇥\n");
        let result = refusing.ask(many()).map(|got| names(&got));
        assert_eq!(outcome(&result), "date");
        let frames = refusing.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains("▲  Which?\n")),
            "{frames:?}"
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.contains("└  Please select at least one item."))
                .count(),
            1,
            "{frames:?}"
        );
    }

    #[test]
    fn no_options_make_a_typed_error_and_no_question() {
        let form = live("\n");
        let single =
            form.ask(autocomplete("Which?").items(Vec::<Choice<Fruit>>::new()));
        assert!(matches!(single, Err(Error::NoChoices)), "{single:?}");
        let multi = form
            .ask(autocomplete_multiselect("Which?").items(Vec::<Choice<Fruit>>::new()));
        assert!(matches!(multi, Err(Error::NoChoices)), "{multi:?}");
        assert_eq!(form.terminal().written(), "");
    }

    #[test]
    fn items_take_tuples_and_hints_apply_to_the_last() -> Result<()> {
        let prompt = autocomplete("Which?")
            .item(1_u8, "One")
            .hint("first")
            .items([(2, "Two", "second"), (3, "Three", "third")]);
        let shown = rendered(&prompt, "");
        assert_eq!(shown.get(1).map(String::as_str), Some("│  ● One (first)"));
        assert_eq!(prompt.simulate(keys("thr\n"))?, 3);
        Ok(())
    }

    #[test]
    fn peek_reads_the_value_under_the_cursor() {
        let prompt = one();
        let state = after(&prompt, keys("ban"));
        let peeked = state
            .as_ref()
            .and_then(|state| prompt.peek(state))
            .map(|fruit| fruit.0);
        assert_eq!(peeked, Some("banana"));
    }

    #[test]
    fn validate_keeps_the_question_open_under_a_warning() -> Result<()> {
        let form = live("\n↓\n");
        let prompt = one().validate(|fruit| {
            if fruit.0 == "apple" {
                Err("no apples today")
            } else {
                Ok(())
            }
        });
        assert_eq!(form.ask(prompt)?.0, "banana");
        let frames = form.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains("▲  Which?\n")),
            "{frames:?}"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.contains("└  no apples today")),
            "{frames:?}"
        );
        Ok(())
    }

    #[test]
    fn the_frame_has_the_search_line_the_options_and_the_keys() {
        let written = written_by("␛");
        assert!(
            written.contains(concat!(
                "◆  Which?\n",
                "│  Search: ▏\n",
                "│  ● Apple (red)\n",
                "│  ○ Banana\n",
                "│  ○ Cherry\n",
                "│  ○ Date\n",
                "│  ↑/↓ to select • Enter: confirm • Type: to search\n",
                "└\n",
            )),
            "{written:?}"
        );
    }

    #[rstest]
    #[case::many("a", "│  Search: a▏ (3 matches)\n")]
    #[case::one("ba", "│  Search: ba▏ (1 match)\n")]
    #[case::none("zz", "│  Search: zz▏\n│  No matches found\n│  ↑/↓")]
    fn typing_counts_the_matches_and_says_when_there_are_none(
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let frames = frames_by(&format!("{spec}␛"));
        assert!(
            frames.iter().any(|frame| frame.contains(expected)),
            "{frames:?}"
        );
    }

    #[test]
    fn the_placeholder_shows_in_the_search_line() {
        let form = live("␛");
        let result = form.ask(one().placeholder("type a fruit"));
        assert!(result.is_err());
        let written = form.terminal().written();
        assert!(
            written.contains("│  Search: ▏type a fruit\n"),
            "{written:?}"
        );
    }

    #[test]
    fn long_lists_scroll_with_ellipses() {
        let form = live("↓↓↓↓↓↓␛");
        let result = form.ask(
            autocomplete("Which?")
                .items((0..10).map(|n| Choice::new(n, format!("Item {n}"))))
                .max_rows(NonZeroUsize::MIN.saturating_add(2)),
        );
        assert!(result.is_err());
        let frames = form.terminal().frames();
        assert!(
            frames.iter().any(|frame| {
                frame.contains("│  …\n│  ○ Item 4\n│  ○ Item 5\n│  ● Item 6\n│  …\n")
            }),
            "{frames:?}"
        );
    }

    #[test]
    fn the_multiselect_footer_and_marks_follow_the_mode() {
        let form = live("↓␛");
        assert!(form.ask(many()).is_err());
        let frames = form.terminal().frames();
        let shows = |row: &str| frames.iter().any(|frame| frame.contains(row));
        assert!(
            shows(
                "│  ↑/↓ to navigate • Tab: select • Enter: confirm • Type: to search\n"
            ),
            "{frames:?}"
        );
        assert!(
            shows(
                "│  ↑/↓ to navigate • Space/Tab: select • Enter: confirm • Type: to search\n"
            ),
            "{frames:?}"
        );
        assert!(shows("│ ›◻ Apple (red)\n"), "{frames:?}");
        assert!(shows("│  ◼ Date\n"), "{frames:?}");
    }

    #[test]
    fn colour_paints_the_marks_and_the_matches() {
        let script = Script::new(TerminalKind::Live, keys("a␛"), []);
        let form = Session::new(script, Settings::new(ColorChoice::Colored));
        assert!(form.ask(one()).is_err());
        let written = form.terminal().written();
        assert!(written.contains("\x1b[32m●\x1b[0m Apple"), "{written:?}");
        assert!(
            written.contains("\x1b[2m (3 matches)\x1b[0m"),
            "{written:?}"
        );
    }

    #[test]
    fn the_answer_stays_as_its_label() -> Result<()> {
        let form = live("an↓\n");
        form.ask(one())?;
        let written = screen(&form.terminal().written());
        assert!(written.ends_with("◇  Which?\n│  Banana\n│"), "{written}");
        Ok(())
    }

    #[rstest]
    #[case::none("↓↓↓⇥\n", "none")]
    #[case::three("⇥↓⇥\n", "Apple, Banana, Date")]
    fn the_picked_labels_stay_for_a_few(#[case] spec: &str, #[case] shown: &str) {
        let form = live(spec);
        assert!(form.ask(many().optional()).is_ok());
        let written = screen(&form.terminal().written());
        assert!(
            written.ends_with(&format!("◇  Which?\n│  {shown}\n│")),
            "{written}"
        );
    }

    #[test]
    fn a_count_replaces_the_labels_past_three() {
        let answered = |picked: [usize; 4], count: usize| {
            let form = live("\n");
            let prompt = autocomplete_multiselect("Which?")
                .items(fruits())
                .picked(picked.into_iter().take(count));
            assert!(form.ask(prompt).is_ok());
            screen(&form.terminal().written())
        };
        let three = answered([0, 1, 2, 3], 3);
        assert!(
            three.ends_with("◇  Which?\n│  Apple, Banana, Cherry\n│"),
            "{three}"
        );
        let four = answered([0, 1, 2, 3], 4);
        assert!(
            four.ends_with("◇  Which?\n│  4 items selected\n│"),
            "{four}"
        );
    }

    #[test]
    fn a_cancel_stays_as_the_typed_text() {
        let form = live("ban␛");
        assert!(form.ask(one()).is_err());
        let shown = screen(&form.terminal().written());
        assert!(shown.ends_with("■  Which?\n│  ban\n│"), "{shown}");
    }

    #[rstest]
    #[case::number("3", "cherry")]
    #[case::text("ban", "banana")]
    #[case::unmatched_text("zzz", "cancelled")]
    #[case::number_past_the_list("9", "cancelled")]
    fn a_line_autocomplete_takes_a_number_or_a_name(
        #[case] line: &'static str,
        #[case] expected: &str,
    ) {
        let form = bare(Script::new(TerminalKind::Dumb, [], [line]));
        let result = form.ask(one()).map(|fruit| fruit.0);
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[rstest]
    #[case::numbers("1 2", "apple+banana")]
    #[case::none_keeps_the_marked("", "date")]
    #[case::words("all", "cancelled")]
    fn a_line_multiselect_takes_numbers(
        #[case] line: &'static str,
        #[case] expected: &str,
    ) {
        let form = bare(Script::new(TerminalKind::Dumb, [], [line]));
        let result = form.ask(many().optional()).map(|got| names(&got));
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[test]
    fn a_line_autocomplete_lists_the_options() {
        let form = bare(Script::new(TerminalKind::Dumb, [], ["2"]));
        assert!(form.ask(one()).is_ok());
        assert_eq!(
            form.terminal().written(),
            "◆  Which?\n  1) Apple (red)\n  2) Banana\n  3) Cherry (sour)\n  4) Date\n  a number, or part of a name\n◇  Which?\n│  Banana\n│\n"
        );
    }

    fn gaps() -> Vec<Choice<&'static str>> {
        vec![
            Choice::new("apple", "Apple"),
            Choice::disabled("banana", "Banana").hint("soon"),
            Choice::new("cherry", "Cherry"),
            Choice::disabled("date", "Date"),
        ]
    }

    fn gap_one() -> Autocomplete<'static, &'static str> {
        autocomplete("Which?").items(gaps())
    }

    fn gap_many() -> AutocompleteMultiSelect<'static, &'static str> {
        autocomplete_multiselect("Which?")
            .items(gaps())
            .picked([0, 1, 3])
            .optional()
    }

    #[rstest]
    #[case::first_open("\n", "apple")]
    #[case::down_skips("↓\n", "cherry")]
    #[case::down_wraps_over_the_last("↓↓\n", "apple")]
    #[case::up_wraps_over_the_last("↑\n", "cherry")]
    #[case::typing_lands_on_the_first_open_match("a↓\n", "apple")]
    #[case::a_search_of_disabled_ones_only_answers_nothing("ba\n", "no input")]
    #[case::then_widened_by_backspace("ba⌫⌫\n", "apple")]
    fn the_cursor_skips_disabled_options_and_wraps(
        #[case] spec: &str,
        #[case] expected: &str,
    ) {
        let result = gap_one().simulate(keys(spec));
        assert_eq!(outcome(&result), expected, "{result:?}");
    }

    #[test]
    fn only_disabled_options_are_no_choices() {
        let one = autocomplete("Which?")
            .disabled(1, "A")
            .simulate([Key::Enter]);
        assert!(matches!(one, Err(Error::NoChoices)), "{one:?}");
        let many = autocomplete_multiselect("Which?")
            .disabled(1, "A")
            .simulate([Key::Enter]);
        assert!(matches!(many, Err(Error::NoChoices)), "{many:?}");
    }

    #[rstest]
    #[case::tab_drops_the_marked("⇥\n", "")]
    #[case::down_skips("↓⇥\n", "apple+cherry")]
    #[case::down_wraps("↓↓⇥\n", "")]
    fn a_multiselect_cursor_skips_disabled_options_and_picks_open_ones(
        #[case] spec: &str,
        #[case] expected: &str,
    ) -> Result<()> {
        let picked = gap_many().simulate(keys(spec))?;
        assert_eq!(picked.join("+"), expected);
        Ok(())
    }

    #[test]
    fn a_disabled_option_in_picked_starts_unpicked() -> Result<()> {
        assert_eq!(gap_many().simulate(keys("\n"))?, ["apple"]);
        Ok(())
    }

    #[test]
    fn a_typed_line_naming_a_disabled_option_is_refused_with_a_text() {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let one = ui.ask(gap_one().given_line(Some("2")));
        assert!(
            matches!(&one, Err(Error::Refused { reason, .. })
                if *reason == Messages::english().option_disabled),
            "{one:?}"
        );
        for line in ["2", "1 4"] {
            let many = ui.ask(gap_many().given_line(Some(line)));
            assert!(
                matches!(&many, Err(Error::Refused { reason, .. })
                    if *reason == Messages::english().option_disabled),
                "{line}: {many:?}"
            );
        }
    }

    #[test]
    fn a_keyless_terminal_refuses_a_name_that_matches_only_disabled_options()
    -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["pro", "free"]));
        let plan = autocomplete("Plan")
            .item("free", "Free")
            .disabled("pro", "Pro");
        assert_eq!(ui.ask(plan)?, "free");
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.contains(&*Messages::english().option_disabled),
            "{shown}"
        );
        Ok(())
    }

    #[test]
    fn tab_leaves_a_placeholder_that_matches_only_disabled_options() -> Result<()> {
        let plan = autocomplete("Plan")
            .item("free", "Free")
            .disabled("pro", "Pro")
            .placeholder("pro");
        let found = plan.simulate(keys("⇥\n"))?;
        assert_eq!(found, "free");
        Ok(())
    }

    #[test]
    fn a_keyless_terminal_marks_disabled_options_and_walks_past_them() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["2", "3"]));
        assert_eq!(ui.ask(gap_one())?, "cherry");
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.contains(
                "  1) Apple\n  2) [-] Banana (soon)\n  3) Cherry\n  4) [-] Date"
            ),
            "{shown}"
        );
        assert!(shown.contains("That option is disabled."), "{shown}");
        let multi = bare(Script::new(TerminalKind::Dumb, [], ["1 3"]));
        assert_eq!(multi.ask(gap_many())?, ["apple", "cherry"]);
        let listed = screen(&multi.terminal().written());
        assert!(
            listed.contains(
                "  1) [x] Apple\n  2) [-] Banana (soon)\n  3) [ ] Cherry\n  4) [-] Date"
            ),
            "{listed}"
        );
        Ok(())
    }

    #[test]
    fn a_typed_line_unpicks_marked_options_behind_disabled_ones() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["3"]));
        assert_eq!(ui.ask(gap_many())?, ["cherry"]);
        Ok(())
    }

    #[test]
    fn a_given_disabled_value_is_refused_and_an_open_one_taken() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let one = ui.ask(gap_one().given(Some("banana")));
        assert!(matches!(one, Err(Error::Refused { .. })), "{one:?}");
        assert_eq!(ui.ask(gap_one().given(Some("cherry")))?, "cherry");
        let many = ui.ask(gap_many().given(Some(vec!["apple", "date"])));
        assert!(matches!(many, Err(Error::Refused { .. })), "{many:?}");
        assert_eq!(
            ui.ask(gap_many().given(Some(vec!["apple", "cherry"])))?,
            ["apple", "cherry"]
        );
        Ok(())
    }

    #[test]
    fn skip_if_single_counts_the_open_options() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let only = autocomplete("Which?")
            .disabled(1, "A")
            .item(2, "B")
            .disabled(3, "C")
            .skip_if_single();
        assert_eq!(ui.ask(only)?, 2);
        Ok(())
    }

    #[test]
    fn a_disabled_option_looks_dim_and_struck_and_shows_its_hint() {
        let options =
            |rows: Vec<String>| rows.into_iter().skip(1).take(4).collect::<Vec<_>>();
        assert_eq!(
            options(rendered(&gap_one(), "")),
            [
                "│  ● Apple",
                "│  ○ Banana (soon)",
                "│  ○ Cherry",
                "│  ○ Date"
            ]
        );
        assert_eq!(
            options(rendered(&gap_many(), "")),
            [
                "│ ›◼ Apple",
                "│  ◻ Banana (soon)",
                "│  ◻ Cherry",
                "│  ◻ Date"
            ]
        );
        let mut frame = Frame::new(ColorChoice::Colored, Some(60));
        let prompt = gap_one();
        if let Ok(state) = prompt.start() {
            prompt.view(&state, Phase::Active, &mut frame);
        }
        let banana = frame.rows().get(2).cloned().unwrap_or_default();
        assert!(banana.contains("\x1b[2m\x1b[9mBanana\x1b[0m"), "{banana:?}");
    }
}
