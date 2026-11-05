//! Prompts as pure machines: a key gives the next state; no IO, no clock.

use crate::{
    core::ask::Phase,
    render::{
        color::ColorChoice,
        messages::{Messages, Refusal},
        style::Skin,
    },
    values::line::Cancellation,
};

pub(crate) const CHOICE_ROWS: usize = 8;

/// What a key did to a prompt `M` whose answer is `A`.
#[derive(Debug)]
#[doc(hidden)]
pub enum Step<M, A> {
    /// The key edited the prompt; keep going with the new state.
    Editing(M),
    /// The key gave the answer.
    Submit(A),
    /// The key backed out of the question.
    Quit(Cancellation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Availability {
    Open,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) label: String,
    pub(crate) hint: Option<String>,
    pub(crate) availability: Availability,
}

impl Entry {
    pub(crate) const fn open(label: String, hint: Option<String>) -> Self {
        Self {
            label,
            hint,
            availability: Availability::Open,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        matches!(self.availability, Availability::Open)
    }

    pub(crate) fn hinted(&self, wrap: impl Fn(&str) -> String) -> String {
        hinted(self.hint.as_deref(), wrap)
    }

    pub(crate) const fn tick(&self, on: bool) -> &'static str {
        if self.is_open() { tick(on) } else { "[-]" }
    }

    pub(crate) const fn dash(&self) -> &'static str {
        if self.is_open() { "" } else { "[-] " }
    }
}

fn hinted(hint: Option<&str>, wrap: impl Fn(&str) -> String) -> String {
    hint.iter()
        .map(|hint| wrap(&format!(" ({hint})")))
        .collect()
}

/// How a choice prompt tells whether a given answer is one of its values.
#[doc(hidden)]
pub trait Same<V> {
    /// Whether `a` and `b` are the same option.
    #[must_use]
    fn same(&self, a: &V, b: &V) -> bool;
}

/// Values are the same option when they are equal: the default of a hand-built
/// choice, which needs `V: PartialEq` to take a given answer.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct ByEq;

impl<V: PartialEq> Same<V> for ByEq {
    fn same(&self, a: &V, b: &V) -> bool {
        a == b
    }
}

/// Values are the same option when they have the same label: what a derived
/// enum uses, so it needs no `PartialEq`.
#[derive(Debug)]
#[doc(hidden)]
pub struct ByLabel<V>(pub(crate) fn(&V) -> &'static str);

impl<V> Same<V> for ByLabel<V> {
    fn same(&self, a: &V, b: &V) -> bool {
        (self.0)(a) == (self.0)(b)
    }
}

/// An option of a [`Select`](crate::Select) or
/// [`MultiSelect`](crate::MultiSelect): a value, its label and an optional
/// hint.
#[derive(Debug)]
pub struct Choice<V> {
    value: V,
    label: String,
    hint: Option<String>,
    availability: Availability,
}

impl<V> Choice<V> {
    /// An option holding `value`, shown as `label`.
    #[must_use]
    pub fn new(value: V, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            hint: None,
            availability: Availability::Open,
        }
    }

    /// An option the cursor skips and no key picks. It shows dim and struck
    /// through; a [`hint`](Self::hint) can say why.
    #[must_use]
    pub fn disabled(value: V, label: impl Into<String>) -> Self {
        Self {
            availability: Availability::Disabled,
            ..Self::new(value, label)
        }
    }

    /// Shows dim after the option while the cursor is on it; empty is none.
    #[must_use]
    pub fn hint(self, hint: impl Into<String>) -> Self {
        Self {
            hint: Some(hint.into()).filter(|hint| !hint.is_empty()),
            ..self
        }
    }

    pub(crate) fn map<W>(self, f: impl FnOnce(V) -> W) -> Choice<W> {
        Choice {
            value: f(self.value),
            label: self.label,
            hint: self.hint,
            availability: self.availability,
        }
    }

    pub(crate) fn split(
        choices: impl IntoIterator<Item = Self>,
    ) -> (Vec<V>, Vec<Entry>) {
        choices
            .into_iter()
            .map(|choice| {
                (
                    choice.value,
                    Entry {
                        label: choice.label,
                        hint: choice.hint,
                        availability: choice.availability,
                    },
                )
            })
            .unzip()
    }
}

impl<V> From<(V, &str)> for Choice<V> {
    fn from((value, label): (V, &str)) -> Self {
        Self::new(value, label)
    }
}

impl<V> From<(V, &str, &str)> for Choice<V> {
    fn from((value, label, hint): (V, &str, &str)) -> Self {
        Self::new(value, label).hint(hint)
    }
}

#[derive(Debug)]
pub(crate) struct Options<V> {
    pub(crate) values: Vec<V>,
    pub(crate) entries: Vec<Entry>,
}

impl<V> Options<V> {
    pub(crate) const fn new() -> Self {
        Self {
            values: Vec::new(),
            entries: Vec::new(),
        }
    }

    pub(crate) fn extend<C: Into<Choice<V>>>(
        mut self,
        choices: impl IntoIterator<Item = C>,
    ) -> Self {
        let (values, entries) = Choice::split(choices.into_iter().map(Into::into));
        self.values.extend(values);
        self.entries.extend(entries);
        self
    }

    pub(crate) fn hint(mut self, hint: impl Into<String>) -> Self {
        if let Some(last) = self.entries.last_mut() {
            last.hint = Some(hint.into()).filter(|hint| !hint.is_empty());
        }
        self
    }

    pub(crate) fn label(&self, at: usize) -> &str {
        self.entries
            .get(at)
            .map_or("", |option| option.label.as_str())
    }

    pub(crate) fn open(&self, at: usize) -> bool {
        self.entries.get(at).is_some_and(Entry::is_open)
    }

    pub(crate) fn open_places(&self) -> impl Iterator<Item = usize> {
        (0..)
            .zip(&self.entries)
            .filter_map(|(at, entry)| entry.is_open().then_some(at))
    }

    /// `None` when no option can be picked.
    pub(crate) fn settle(&self, start: usize) -> Option<usize> {
        let count = self.entries.len();
        let from = start.min(count.checked_sub(1)?);
        (0..count)
            .map(|step| wrapped(from, step, count))
            .find(|at| self.open(*at))
    }

    pub(crate) fn seek(&self, from: usize, step: usize) -> usize {
        seek(from, step, self.entries.len(), |at| self.open(at))
    }

    pub(crate) fn presses(&self, from: usize, to: usize) -> usize {
        let count = self.entries.len();
        (1..count)
            .map(|step| wrapped(from, step, count))
            .filter(|at| self.open(*at))
            .position(|at| at == to)
            .map_or(0, |before| before.saturating_add(1))
    }
}

pub(crate) fn seek(
    from: usize,
    step: usize,
    count: usize,
    rests: impl Fn(usize) -> bool,
) -> usize {
    (1..=count)
        .map(|turn| wrapped(from, step.saturating_mul(turn), count))
        .find(|at| rests(*at))
        .unwrap_or(from)
}

pub(crate) fn refused_disabled(
    numbers: &[usize],
    open: impl Fn(usize) -> bool,
    messages: &Messages,
) -> Option<Refusal> {
    numbers
        .iter()
        .any(|number| !open(number.saturating_sub(1)))
        .then(|| Refusal::Said(messages.option_disabled.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyPick {
    Allowed,
    Refused,
}

pub(crate) fn wrapped(cursor: usize, step: usize, count: usize) -> usize {
    cursor.saturating_add(step).checked_rem(count).unwrap_or(0)
}

pub(crate) fn rail(skin: &Skin, phase: Phase<'_>) -> String {
    let theme = skin.theme();
    let style = match phase {
        Phase::Warned(_) => theme.warning,
        Phase::Active | Phase::Submitted | Phase::Cancelled => theme.active,
    };
    skin.paint(style, skin.symbols().bar)
}

pub(crate) fn scrolled<T>(
    skin: &Skin,
    bar: &str,
    items: &[T],
    (cursor, rows): (usize, usize),
    row: impl Fn(usize, &T) -> String,
) -> Vec<String> {
    let start = cursor
        .saturating_sub(rows / 2)
        .min(items.len().saturating_sub(rows));
    let end = start.saturating_add(rows).min(items.len());
    let more = |hidden: bool| {
        hidden
            .then(|| {
                format!(
                    "{bar}  {}",
                    skin.paint(skin.theme().hint, skin.symbols().ellipsis)
                )
            })
            .into_iter()
    };
    let shown = (start..)
        .zip(items.get(start..end).unwrap_or_default())
        .map(|(at, item)| row(at, item));
    more(start > 0)
        .chain(shown)
        .chain(more(end < items.len()))
        .collect()
}

pub(crate) fn disabled_text(skin: &Skin, entry: &Entry) -> String {
    let theme = skin.theme();
    format!(
        "{}{}",
        skin.paint(theme.disabled, &entry.label),
        entry.hinted(|hint| skin.paint(theme.hint, hint))
    )
}

pub(crate) fn radio_row(
    skin: &Skin,
    here: bool,
    label: &str,
    hint: Option<&str>,
) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    if here {
        format!(
            "{} {label}{}",
            skin.paint(theme.selected, symbols.radio_active),
            hinted(hint, |hint| skin.paint(theme.hint, hint))
        )
    } else {
        skin.paint(
            theme.unselected,
            &format!("{} {label}", symbols.radio_inactive),
        )
    }
}

pub(crate) fn radio_option(skin: &Skin, here: bool, entry: &Entry) -> String {
    if entry.is_open() {
        radio_row(skin, here, &entry.label, entry.hint.as_deref())
    } else {
        format!(
            "{} {}",
            skin.paint(skin.theme().unselected, skin.symbols().radio_inactive),
            disabled_text(skin, entry)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fill {
    Nothing,
    Some,
    All,
}

impl From<bool> for Fill {
    fn from(on: bool) -> Self {
        if on { Self::All } else { Self::Nothing }
    }
}

pub(crate) fn checkbox(skin: &Skin, fill: Fill, lit: bool) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    match (fill, lit) {
        (Fill::All, _) => skin.paint(theme.selected, symbols.checkbox_selected),
        (Fill::Some, _) => skin.paint(theme.selected, symbols.checkbox_partial),
        (Fill::Nothing, true) => skin.paint(theme.active, symbols.checkbox_active),
        (Fill::Nothing, false) => {
            skin.paint(theme.unselected, symbols.checkbox_inactive)
        }
    }
}

/// Under Plain the cursor row leads with `›` and shows its hint; a disabled
/// option always does.
pub(crate) fn checkbox_row(
    skin: &Skin,
    bar: &str,
    (here, on): (bool, bool),
    entry: &Entry,
) -> String {
    let (theme, symbols) = (skin.theme(), skin.symbols());
    let lead = match (skin.color(), here) {
        (ColorChoice::Plain, true) => format!(" {}", symbols.cursor),
        (ColorChoice::Plain | ColorChoice::Colored, _) => "  ".to_owned(),
    };
    let text = if entry.is_open() {
        let hint = hinted(entry.hint.as_deref().filter(|_| here), |hint| {
            skin.paint(theme.hint, hint)
        });
        format!("{}{hint}", entry.label)
    } else {
        disabled_text(skin, entry)
    };
    let fill = Fill::from(on && entry.is_open());
    format!("{bar}{lead}{} {text}", checkbox(skin, fill, here))
}

pub(crate) const fn tick(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
}

pub(crate) fn named(line: &str, count: usize) -> Option<Vec<usize>> {
    line.split([',', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            word.parse::<usize>()
                .ok()
                .filter(|number| (1..=count).contains(number))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        core::ask::{Frame, Phase, Prompt},
        prompts::{
            confirm::confirm,
            group_multiselect::group_multiselect,
            multiselect::multiselect,
            select::select,
            select_key::select_key,
        },
        render::{color::ColorChoice, style::Skin},
    };

    fn first_row<P: Prompt>(prompt: &P, phase: Phase<'_>) -> String {
        let mut frame = Frame::new(ColorChoice::Colored, Some(80));
        if let Ok(state) = prompt.start() {
            prompt.view(&state, phase, &mut frame);
        }
        frame.rows().first().cloned().unwrap_or_default()
    }

    fn first_rows(phase: Phase<'_>) -> [String; 5] {
        [
            first_row(&select("Q").item(1, "One"), phase),
            first_row(&multiselect("Q").item(1, "One"), phase),
            first_row(&group_multiselect("Q").group("G", [(1, "One")]), phase),
            first_row(&select_key("Q").item('o', 1, "One"), phase),
            first_row(&confirm("Q"), phase),
        ]
    }

    #[test]
    fn the_rail_of_an_open_choice_is_active_and_warned_after_a_refusal() {
        let skin = Skin::new(ColorChoice::Colored);
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let active = skin.paint(theme.active, symbols.bar);
        let warning = skin.paint(theme.warning, symbols.bar);
        for row in first_rows(Phase::Active) {
            assert!(row.starts_with(&active), "{row:?}");
        }
        for row in first_rows(Phase::Warned("x")) {
            assert!(row.starts_with(&warning), "{row:?}");
        }
    }

    #[test]
    fn seek_wraps_skips_and_stays_put_when_nothing_rests() {
        let open = |at: usize| at % 3 == 0;
        assert_eq!(super::seek(0, 1, 7, open), 3);
        assert_eq!(super::seek(6, 1, 7, open), 0);
        assert_eq!(super::seek(0, 6, 7, open), 6);
        assert_eq!(super::seek(4, 6, 7, open), 3);
        assert_eq!(super::seek(2, 1, 7, |_| false), 2);
        assert_eq!(super::seek(0, 1, 0, open), 0);
    }

    #[test]
    fn one_row_shows_the_option_under_the_cursor() {
        let skin = Skin::new(ColorChoice::Plain);
        let items = ["A", "B", "C"];
        let shown = super::scrolled(&skin, "|", &items, (1, 1), |_, item| {
            (*item).to_owned()
        });
        assert!(shown.iter().any(|row| row == "B"), "{shown:?}");
    }
}
