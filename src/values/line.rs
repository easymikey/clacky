//! Pure line editor: a [`TypedAnswer`] and a [`Key`] give the next answer.

#[cfg(test)] mod tests;

use unicode_segmentation::UnicodeSegmentation as _;
use zeroize::Zeroizing;

const CAPACITY: usize = 256;

/// Which characters an answer takes; control characters never.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CharFilter {
    /// Anything printable, spaces included.
    #[default]
    Printable,
    /// Anything printable but whitespace, so a pasted `123 456` is `123456`.
    NoSpaces,
}

impl CharFilter {
    fn keeps(self, c: char) -> bool {
        !c.is_control()
            && match self {
                Self::Printable => true,
                Self::NoSpaces => !c.is_whitespace(),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One key press, already decoded.
pub enum Key {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Tab.
    Tab,
    /// Moves a word left.
    WordLeft,
    /// Moves a word right.
    WordRight,
    /// Moves to the start of the line.
    Home,
    /// Moves to the end of the line.
    End,
    /// Deletes everything before the cursor.
    ClearBefore,
    /// Deletes everything after the cursor.
    ClearAfter,
    /// Deletes the word before the cursor.
    DeleteWord,
    /// Ctrl-D: the end of input.
    EndOfInput,
    /// Ctrl-C.
    Interrupt,
    /// Esc on its own.
    Cancel,
    /// A key nothing uses.
    Ignored,
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Csi {
    parameters: [u16; 2],
    at: usize,
}

#[cfg(any(unix, test))]
impl Csi {
    #[must_use]
    pub(crate) fn feed(self, c: char) -> Self {
        match (c.to_digit(10), c) {
            (Some(digit), _) => Self {
                parameters: with_digit(self.parameters, self.at, digit),
                ..self
            },
            (None, ';') => Self {
                at: self.at.saturating_add(1),
                ..self
            },
            (None, _) => self,
        }
    }

    #[must_use]
    pub(crate) fn key(self, last: char) -> Key {
        let [first, modifier] = self.parameters;
        let by_word = matches!(modifier, 3 | 5 | 7 | 9);
        match (last, by_word, first) {
            ('D', true, _) => Key::WordLeft,
            ('C', true, _) => Key::WordRight,
            ('D', false, _) => Key::Left,
            ('C', false, _) => Key::Right,
            ('A', _, _) => Key::Up,
            ('B', _, _) => Key::Down,
            ('H', _, _) | ('~', _, 1 | 7) => Key::Home,
            ('F', _, _) | ('~', _, 4 | 8) => Key::End,
            ('~', _, 3) => Key::Delete,
            _ => Key::Ignored,
        }
    }
}

#[cfg(any(unix, test))]
fn with_digit(mut numbers: [u16; 2], at: usize, digit: u32) -> [u16; 2] {
    numbers.get_mut(at).into_iter().for_each(|param| {
        *param = param
            .saturating_mul(10)
            .saturating_add(u16::try_from(digit).unwrap_or(0));
    });
    numbers
}

#[cfg(any(unix, test))]
#[must_use]
pub(crate) const fn ss3_key(last: char) -> Key {
    match last {
        'H' => Key::Home,
        'F' => Key::End,
        'D' => Key::Left,
        'C' => Key::Right,
        'A' => Key::Up,
        'B' => Key::Down,
        _ => Key::Ignored,
    }
}

#[derive(Debug)]
/// A line being typed, with its cursor.
pub struct TypedAnswer {
    filter: CharFilter,
    text: Zeroizing<String>,
    cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// How typing ended without an answer.
pub enum Cancellation {
    /// Ctrl-C.
    Interrupt,
    /// Esc or Ctrl-D.
    Cancel,
}

impl TypedAnswer {
    /// An empty line that keeps only the characters `filter` allows.
    #[must_use]
    pub fn new(filter: CharFilter) -> Self {
        Self {
            filter,
            text: Zeroizing::new(String::with_capacity(CAPACITY)),
            cursor: 0,
        }
    }

    /// Starts with `initial` typed, the cursor after it.
    #[must_use]
    pub fn prefilled(self, initial: &str) -> Self {
        let filter = self.filter;
        initial
            .chars()
            .filter(|c| filter.keeps(*c))
            .fold(self, Self::inserted)
    }

    /// The text typed so far.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// How many characters stand before the cursor.
    #[must_use]
    pub fn graphemes_before(&self) -> usize {
        boundaries(&self.text)
            .take_while(|at| *at < self.cursor)
            .count()
    }

    #[must_use]
    pub(crate) fn edit(self, keystroke: Key) -> Self {
        let (text, at) = (&*self.text, self.cursor);
        match keystroke {
            Key::Backspace => {
                let from = before(text, at);
                self.cut(from, at)
            }
            Key::Delete => {
                let to = after(text, at);
                self.cut(at, to)
            }
            Key::ClearBefore => self.cut(0, at),
            Key::ClearAfter => {
                let end = text.len();
                self.cut(at, end)
            }
            Key::DeleteWord => {
                let from = spaced_word_start(text, at);
                self.cut(from, at)
            }
            Key::Left => self.moved_by(before),
            Key::Right => self.moved_by(after),
            Key::WordLeft => self.moved_by(word_start),
            Key::WordRight => self.moved_by(word_end),
            Key::Home => self.moved(0),
            Key::End => {
                let end = text.len();
                self.moved(end)
            }
            Key::Char(c) if self.filter.keeps(c) => self.inserted(c),
            Key::Char(_)
            | Key::Enter
            | Key::Interrupt
            | Key::Cancel
            | Key::EndOfInput
            | Key::Up
            | Key::Down
            | Key::Tab
            | Key::Ignored => self,
        }
    }

    fn moved_by(self, target: fn(&str, usize) -> usize) -> Self {
        let to = target(&self.text, self.cursor);
        self.moved(to)
    }

    #[must_use]
    pub(crate) fn trimmed(self) -> Self {
        let text = self.answer();
        Self {
            cursor: text.len(),
            text,
            ..self
        }
    }

    #[must_use]
    pub(crate) fn into_text(self) -> Zeroizing<String> {
        self.text
    }

    fn answer(&self) -> Zeroizing<String> {
        Zeroizing::new(self.text.trim().to_owned())
    }

    pub(crate) fn moved(self, to: usize) -> Self {
        Self { cursor: to, ..self }
    }

    pub(crate) fn inserted(self, c: char) -> Self {
        let text = inserted_at(with_room(self.text, c.len_utf8()), self.cursor, c);
        let cursor = snap(&text, self.cursor + c.len_utf8());
        Self {
            text,
            cursor,
            ..self
        }
    }

    pub(crate) fn cut(self, from: usize, to: usize) -> Self {
        let text = without_range(self.text, from, to);
        let cursor = snap(&text, from);
        Self {
            text,
            cursor,
            ..self
        }
    }
}

fn inserted_at(mut text: Zeroizing<String>, at: usize, c: char) -> Zeroizing<String> {
    text.insert(at, c);
    text
}

fn without_range(
    mut text: Zeroizing<String>,
    from: usize,
    to: usize,
) -> Zeroizing<String> {
    text.replace_range(from..to, "");
    text
}

#[cfg(unix)]
#[must_use]
pub(crate) const fn csi_parameter(c: char) -> bool {
    matches!(c, ' '..='?')
}

#[cfg(unix)]
#[must_use]
pub(crate) const fn csi_final(c: char) -> bool {
    matches!(c, '@'..='~')
}

fn boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .chain([text.len()])
}

fn snap(text: &str, at: usize) -> usize {
    boundaries(text).find(|b| *b >= at).unwrap_or(text.len())
}

fn before(text: &str, at: usize) -> usize {
    boundaries(text).take_while(|b| *b < at).last().unwrap_or(0)
}

fn after(text: &str, at: usize) -> usize {
    boundaries(text).find(|b| *b > at).unwrap_or(text.len())
}

fn is_word(grapheme: &str) -> bool {
    grapheme.chars().next().is_some_and(char::is_alphanumeric)
}

fn word_start(text: &str, at: usize) -> usize {
    let back: Vec<(usize, &str)> = text
        .grapheme_indices(true)
        .take_while(|(start, _)| *start < at)
        .collect();
    back.iter()
        .rev()
        .skip_while(|(_, g)| !is_word(g))
        .take_while(|(_, g)| is_word(g))
        .last()
        .map_or(0, |(start, _)| *start)
}

fn word_end(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .skip_while(|(start, _)| *start < at)
        .skip_while(|(_, g)| !is_word(g))
        .take_while(|(_, g)| is_word(g))
        .last()
        .map_or(text.len(), |(start, g)| start + g.len())
}

fn spaced_word_start(text: &str, at: usize) -> usize {
    let back: Vec<(usize, &str)> = text
        .grapheme_indices(true)
        .take_while(|(start, _)| *start < at)
        .collect();
    let blank = |g: &str| g.chars().all(char::is_whitespace);
    back.iter()
        .rev()
        .skip_while(|(_, g)| blank(g))
        .take_while(|(_, g)| !blank(g))
        .last()
        .map_or(0, |(start, _)| *start)
}

fn with_room(text: Zeroizing<String>, extra: usize) -> Zeroizing<String> {
    match text.capacity().checked_sub(text.len()) {
        Some(room) if room >= extra => text,
        _ => {
            let size = text.capacity().saturating_mul(2).max(text.len() + extra);
            Zeroizing::new(String::with_capacity(size) + &text)
        }
    }
}
