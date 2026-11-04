//! A map from keys to prompt actions that a program can rebind.

use crate::values::line::Key;

/// What a key can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Moves the cursor up.
    Up,
    /// Moves the cursor down.
    Down,
    /// Moves the cursor left.
    Left,
    /// Moves the cursor right.
    Right,
    /// Ticks the option under the cursor.
    Toggle,
    /// Ticks every option, or none when all are ticked.
    ToggleAll,
    /// Gives the answer.
    Submit,
    /// Backs out of the question.
    Cancel,
    /// Takes the offered completion; a choice moves down.
    Complete,
}

impl Action {
    const fn key(self) -> Key {
        match self {
            Self::Up => Key::Up,
            Self::Down => Key::Down,
            Self::Left => Key::Left,
            Self::Right => Key::Right,
            Self::Toggle => Key::Char(' '),
            Self::ToggleAll => Key::Char('a'),
            Self::Submit => Key::Enter,
            Self::Cancel => Key::Cancel,
            Self::Complete => Key::Tab,
        }
    }
}

/// What a prompt does with a printable key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Printable {
    /// Text input; the map leaves it alone.
    #[default]
    Typed,
    /// It may be bound to an action, like `j` for down.
    Bound,
}

/// The keys of a session and what they do.
///
/// ```
/// use clacky::{
///     core::keys::{Action, KeyMap},
///     prelude::Key,
/// };
/// let keys = KeyMap::clack()
///     .unbind(Key::Char('j'))
///     .bind(Key::Char('n'), Action::Down);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMap {
    binds: Vec<(Key, Action)>,
}

impl KeyMap {
    /// The arrows, Tab, Space, Enter and Esc, plus clack's aliases: `k` `j`
    /// `h` `l` for the arrows and `a` for all.
    #[must_use]
    pub fn clack() -> Self {
        Self {
            binds: vec![
                (Key::Up, Action::Up),
                (Key::Down, Action::Down),
                (Key::Left, Action::Left),
                (Key::Right, Action::Right),
                (Key::Char('k'), Action::Up),
                (Key::Char('j'), Action::Down),
                (Key::Char('h'), Action::Left),
                (Key::Char('l'), Action::Right),
                (Key::Char(' '), Action::Toggle),
                (Key::Char('a'), Action::ToggleAll),
                (Key::Enter, Action::Submit),
                (Key::Cancel, Action::Cancel),
                (Key::Tab, Action::Complete),
            ],
        }
    }

    /// Binds `key` to `action`, replacing any earlier binding. Ctrl-C and
    /// Ctrl-D cannot be rebound.
    #[must_use]
    pub fn bind(self, key: Key, action: Action) -> Self {
        let mut keys = self.unbind(key);
        keys.binds.push((key, action));
        keys
    }

    /// Removes the binding of `key`; prompts then ignore it.
    #[must_use]
    pub fn unbind(mut self, key: Key) -> Self {
        self.binds.retain(|(bound, _)| *bound != key);
        self
    }

    /// The action bound to `key`, if any.
    #[must_use]
    pub fn action(&self, key: Key) -> Option<Action> {
        self.binds
            .iter()
            .find_map(|(bound, action)| (*bound == key).then_some(*action))
    }

    #[must_use]
    pub(crate) fn translate(&self, key: Key, printable: Printable) -> Key {
        if matches!(key, Key::Interrupt | Key::EndOfInput)
            || matches!(key, Key::Char(_)) && printable == Printable::Typed
        {
            return key;
        }
        match self.action(key) {
            Some(action) => action.key(),
            None if has_action(key) => Key::Ignored,
            None => key,
        }
    }
}

/// Keys a prompt reads as an action when it sees them, so unbinding one
/// must not leave it working.
const fn has_action(key: Key) -> bool {
    matches!(
        key,
        Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Tab
            | Key::Enter
            | Key::Cancel
            | Key::Char(' ' | 'a')
    )
}

#[cfg(test)]
mod tests {
    use super::{Action, Key, KeyMap, Printable};

    #[test]
    fn clack_aliases_become_the_arrows() {
        let keys = KeyMap::clack();
        let bound = Printable::Bound;
        assert_eq!(keys.translate(Key::Char('j'), bound), Key::Down);
        assert_eq!(keys.translate(Key::Char('k'), bound), Key::Up);
        assert_eq!(keys.translate(Key::Char('h'), bound), Key::Left);
        assert_eq!(keys.translate(Key::Char('l'), bound), Key::Right);
        assert_eq!(keys.translate(Key::Char('y'), bound), Key::Char('y'));
    }

    #[test]
    fn typed_text_is_left_alone() {
        let keys = KeyMap::clack();
        assert_eq!(
            keys.translate(Key::Char('j'), Printable::Typed),
            Key::Char('j')
        );
        assert_eq!(
            keys.translate(Key::Char(' '), Printable::Typed),
            Key::Char(' ')
        );
        assert_eq!(keys.translate(Key::Up, Printable::Typed), Key::Up);
    }

    #[test]
    fn an_unbound_key_does_nothing() {
        let keys = KeyMap::clack().unbind(Key::Char('j')).unbind(Key::Down);
        assert_eq!(
            keys.translate(Key::Char('j'), Printable::Bound),
            Key::Char('j')
        );
        assert_eq!(keys.translate(Key::Down, Printable::Bound), Key::Ignored);
        assert_eq!(
            keys.translate(Key::Interrupt, Printable::Bound),
            Key::Interrupt
        );
    }

    #[test]
    fn ctrl_c_and_ctrl_d_cannot_be_rebound() {
        let keys = KeyMap::clack()
            .bind(Key::Interrupt, Action::Submit)
            .bind(Key::EndOfInput, Action::Submit);
        let bound = Printable::Bound;
        assert_eq!(keys.translate(Key::Interrupt, bound), Key::Interrupt);
        assert_eq!(keys.translate(Key::EndOfInput, bound), Key::EndOfInput);
    }

    #[test]
    fn a_rebound_key_takes_the_new_action() {
        let keys = KeyMap::clack().bind(Key::Char('n'), Action::Down);
        assert_eq!(keys.action(Key::Char('n')), Some(Action::Down));
        assert_eq!(keys.translate(Key::Char('n'), Printable::Bound), Key::Down);
        let moved = keys.bind(Key::Char('n'), Action::Up);
        assert_eq!(moved.action(Key::Char('n')), Some(Action::Up));
    }
}
