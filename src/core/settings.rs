//! Session settings: output mode, defaults mode, keys and skin.
use std::{env, ffi::OsStr};

use crate::{
    core::keys::KeyMap,
    render::{color::ColorChoice, style::Skin},
};

/// How the session draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputMode {
    /// Prompts redraw in place and the spinner turns.
    #[default]
    Animated,
    /// No redraws and no cursor moves.
    Accessible,
}

impl OutputMode {
    /// Picks [`Accessible`](Self::Accessible) when `term` is `dumb`, else
    /// [`Animated`](Self::Animated).
    #[must_use]
    pub fn from_vars(term: Option<&OsStr>) -> Self {
        if term.is_some_and(|term| term == "dumb") {
            Self::Accessible
        } else {
            Self::Animated
        }
    }

    /// Reads `TERM` from the process environment.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(env::var_os("TERM").as_deref())
    }
}

/// Whether a session asks its questions or answers them itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Defaults {
    /// Every question is asked.
    #[default]
    Ask,
    /// The `--yes` mode. A question with a default takes it and shows its
    /// answered frame; one without fails with
    /// [`Error::NoDefault`](crate::core::error::Error::NoDefault).
    Take,
}

#[derive(Debug, Clone)]
/// What a session is set up with: look, keys, output mode and defaults mode.
pub struct Settings {
    pub(crate) skin: Skin,
    pub(crate) defaults: Defaults,
    pub(crate) keys: KeyMap,
    pub(crate) output: OutputMode,
}

impl Settings {
    /// Settings with the clack theme, texts, keys and unicode symbols.
    #[must_use]
    pub fn new(color: ColorChoice) -> Self {
        Self {
            skin: Skin::new(color),
            defaults: Defaults::Ask,
            keys: KeyMap::clack(),
            output: OutputMode::Animated,
        }
    }

    /// Sets whether the session asks or takes defaults.
    #[must_use]
    pub fn defaults(self, defaults: Defaults) -> Self {
        Self { defaults, ..self }
    }

    pub(crate) const fn defaults_mode(&self) -> Defaults {
        self.defaults
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::OutputMode;

    #[test]
    fn a_dumb_terminal_is_accessible() {
        assert_eq!(
            OutputMode::from_vars(Some(OsStr::new("dumb"))),
            OutputMode::Accessible
        );
        assert_eq!(
            OutputMode::from_vars(Some(OsStr::new("xterm-256color"))),
            OutputMode::Animated
        );
        assert_eq!(OutputMode::from_vars(None), OutputMode::Animated);
    }
}
