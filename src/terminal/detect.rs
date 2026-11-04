//! Terminal type, stream kind and color hints, from the environment.

use std::{env, ffi::OsStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// What the terminal can do, going by `TERM`.
pub enum TerminalType {
    /// `TERM=dumb`: no cursor movement, no keys.
    Dumb,
    /// Any other terminal.
    Capable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Where a standard stream leads.
pub enum StreamKind {
    /// To a terminal.
    Terminal,
    /// To a pipe or a file.
    Redirected,
}

impl TerminalType {
    /// `Dumb` when `TERM` is `dumb`, else `Capable`.
    #[must_use]
    pub fn from_process() -> Self {
        Self::dumb_if(env::var_os("TERM").is_some_and(|term| term == "dumb"))
    }

    /// `Dumb` when `dumb`, else `Capable`.
    #[must_use]
    pub const fn dumb_if(dumb: bool) -> Self {
        if dumb {
            TerminalType::Dumb
        } else {
            TerminalType::Capable
        }
    }
}

impl StreamKind {
    /// `Terminal` when `terminal`, else `Redirected`.
    #[must_use]
    pub const fn terminal_if(terminal: bool) -> Self {
        if terminal {
            StreamKind::Terminal
        } else {
            StreamKind::Redirected
        }
    }
}

/// What the environment says about color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorHint {
    /// `CLICOLOR_FORCE`: color even into a pipe.
    Forced,
    /// `NO_COLOR`, `CLICOLOR=0` or `TERM=dumb`.
    Refused,
    /// Nothing set: color on a terminal only.
    Open,
}

impl ColorHint {
    /// Reads `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE` and `TERM` from the
    /// process environment.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(
            env::var_os("NO_COLOR").as_deref(),
            env::var_os("CLICOLOR").as_deref(),
            env::var_os("CLICOLOR_FORCE").as_deref(),
            env::var_os("TERM").as_deref(),
        )
    }

    /// The hint for the given variable values; `clicolor_force` beats every
    /// refusal.
    #[must_use]
    pub fn from_vars(
        no_color: Option<&OsStr>,
        clicolor: Option<&OsStr>,
        clicolor_force: Option<&OsStr>,
        term: Option<&OsStr>,
    ) -> Self {
        if clicolor_force.is_some_and(|value| value != "0") {
            Self::Forced
        } else if no_color.is_some_and(|value| !value.is_empty())
            || clicolor.is_some_and(|value| value == "0")
            || term.is_some_and(|value| value == "dumb")
        {
            Self::Refused
        } else {
            Self::Open
        }
    }

    /// Whether color goes to `stream`.
    #[must_use]
    pub const fn colored_on(self, stream: StreamKind) -> bool {
        match (self, stream) {
            (Self::Forced, _) | (Self::Open, StreamKind::Terminal) => true,
            (Self::Refused, _) | (Self::Open, StreamKind::Redirected) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use rstest::rstest;

    use crate::terminal::detect::{ColorHint, StreamKind};

    #[rstest]
    #[case::none(None, None, None, None, ColorHint::Open)]
    #[case::xterm(None, None, None, Some("xterm-256color"), ColorHint::Open)]
    #[case::no_color(Some("1"), None, None, None, ColorHint::Refused)]
    #[case::empty_no_color(Some(""), None, None, None, ColorHint::Open)]
    #[case::clicolor_off(None, Some("0"), None, None, ColorHint::Refused)]
    #[case::clicolor_on(None, Some("1"), None, None, ColorHint::Open)]
    #[case::dumb(None, None, None, Some("dumb"), ColorHint::Refused)]
    #[case::forced(None, None, Some("1"), None, ColorHint::Forced)]
    #[case::force_off(None, None, Some("0"), None, ColorHint::Open)]
    #[case::forced_over_no_color(
        Some("1"),
        Some("0"),
        Some("1"),
        Some("dumb"),
        ColorHint::Forced
    )]
    fn variables_make_the_hint(
        #[case] no_color: Option<&str>,
        #[case] clicolor: Option<&str>,
        #[case] force: Option<&str>,
        #[case] term: Option<&str>,
        #[case] hint: ColorHint,
    ) {
        let vars = ColorHint::from_vars(
            no_color.map(OsStr::new),
            clicolor.map(OsStr::new),
            force.map(OsStr::new),
            term.map(OsStr::new),
        );
        assert_eq!(vars, hint);
    }

    #[rstest]
    #[case::open_terminal(ColorHint::Open, StreamKind::Terminal, true)]
    #[case::open_pipe(ColorHint::Open, StreamKind::Redirected, false)]
    #[case::refused_terminal(ColorHint::Refused, StreamKind::Terminal, false)]
    #[case::forced_pipe(ColorHint::Forced, StreamKind::Redirected, true)]
    fn hint_and_stream_decide_the_color(
        #[case] hint: ColorHint,
        #[case] stream: StreamKind,
        #[case] color: bool,
    ) {
        assert_eq!(hint.colored_on(stream), color);
    }
}
