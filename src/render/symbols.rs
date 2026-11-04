//! The glyphs the library draws, as a value: unicode, or the ASCII set of
//! clack for terminals that cannot show them.

use std::{env, ffi::OsStr};

use crate::render::settings::settings;

/// Whether the rail at the left of the form is drawn (clack's `withGuide`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Guide {
    /// The rail runs down the left, and the intro and outro have their bars.
    #[default]
    Drawn,
    /// No rail and no bars: the rows stand alone.
    Hidden,
}

settings! {
    /// One field per glyph; change one with its builder method.
    ///
    /// ```
    /// use clacky::render::symbols::Symbols;
    /// let symbols = Symbols::unicode().bar("┃").spinner(["-", "\\", "|", "/"]);
    /// assert_eq!(symbols.bar, "┃");
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct Symbols {
        /// The rail.
        bar: &'static str,
        /// The corner that opens the rail.
        bar_start: &'static str,
        /// The corner that closes the rail.
        bar_end: &'static str,
        /// The top right corner of a square box.
        bar_start_right: &'static str,
        /// The bottom right corner of a square box.
        bar_end_right: &'static str,
        /// The horizontal line of a box.
        bar_h: &'static str,
        /// The marker of an open question.
        step_active: &'static str,
        /// The marker of a question backed out of.
        step_cancel: &'static str,
        /// The marker of a question under a warning.
        step_error: &'static str,
        /// The marker of an answered question.
        step_submit: &'static str,
        /// The mark of an information log entry.
        info: &'static str,
        /// The mark of a success log entry.
        success: &'static str,
        /// The mark of a warning log entry.
        warn: &'static str,
        /// The mark of an error log entry.
        error: &'static str,
        /// The option under the cursor in a single choice.
        radio_active: &'static str,
        /// An option not under the cursor in a single choice.
        radio_inactive: &'static str,
        /// An unticked box under the cursor.
        checkbox_active: &'static str,
        /// A ticked box.
        checkbox_selected: &'static str,
        /// An unticked box not under the cursor.
        checkbox_inactive: &'static str,
        /// A group whose options are some of them ticked.
        checkbox_partial: &'static str,
        /// The top left corner of a box.
        corner_top_left: &'static str,
        /// The top right corner of a box.
        corner_top_right: &'static str,
        /// The bottom left corner of a box.
        corner_bottom_left: &'static str,
        /// The bottom right corner of a box.
        corner_bottom_right: &'static str,
        /// Where the rail meets the left side of a box.
        connect_left: &'static str,
        /// Marks the row under the cursor where color cannot.
        cursor: &'static str,
        /// Marks the caret in a line where color cannot.
        caret: &'static str,
        /// What each character of a typed secret shows as.
        mask: &'static str,
        /// Where text was cut, or more rows hide.
        ellipsis: &'static str,
        /// The turning frames of a spinner.
        spinner: [&'static str; 4],
    }
}

impl Symbols {
    /// The glyphs of clack.
    #[must_use]
    pub const fn unicode() -> Self {
        Self {
            bar: "│",
            bar_start: "┌",
            bar_end: "└",
            bar_start_right: "┐",
            bar_end_right: "┘",
            bar_h: "─",
            step_active: "◆",
            step_cancel: "■",
            step_error: "▲",
            step_submit: "◇",
            info: "●",
            success: "◆",
            warn: "▲",
            error: "■",
            radio_active: "●",
            radio_inactive: "○",
            checkbox_active: "◻",
            checkbox_selected: "◼",
            checkbox_inactive: "◻",
            checkbox_partial: "◪",
            corner_top_left: "╭",
            corner_top_right: "╮",
            corner_bottom_left: "╰",
            corner_bottom_right: "╯",
            connect_left: "├",
            cursor: "›",
            caret: "▏",
            mask: "▪",
            ellipsis: "…",
            spinner: ["◒", "◐", "◓", "◑"],
        }
    }

    /// The fallback of clack: ASCII only.
    #[must_use]
    pub const fn ascii() -> Self {
        Self {
            bar: "|",
            bar_start: "T",
            bar_end: "-",
            bar_start_right: "T",
            bar_end_right: "-",
            bar_h: "-",
            step_active: "*",
            step_cancel: "x",
            step_error: "x",
            step_submit: "o",
            info: "*",
            success: "*",
            warn: "!",
            error: "x",
            radio_active: ">",
            radio_inactive: " ",
            checkbox_active: "[*]",
            checkbox_selected: "[+]",
            checkbox_inactive: "[ ]",
            checkbox_partial: "[-]",
            corner_top_left: "+",
            corner_top_right: "+",
            corner_bottom_left: "+",
            corner_bottom_right: "+",
            connect_left: "+",
            cursor: ">",
            caret: "|",
            mask: "*",
            ellipsis: "...",
            spinner: ["*", "o", "O", "0"],
        }
    }

    /// The same glyphs with the rail and its corners blank.
    #[must_use]
    pub const fn without_rail(self) -> Self {
        Self {
            bar: "",
            bar_start: "",
            bar_end: "",
            bar_start_right: "",
            bar_end_right: "",
            connect_left: "",
            ..self
        }
    }

    /// What this process's terminal shows: the host decides, as in clack.
    #[must_use]
    pub fn from_process() -> Self {
        let term = env::var_os("TERM");
        if cfg!(windows) {
            Self::on_windows(
                term.as_deref(),
                env::var_os("WT_SESSION").as_deref(),
                env::var_os("TERM_PROGRAM").as_deref(),
            )
        } else {
            Self::on_unix(term.as_deref())
        }
    }

    /// Unicode, except on the Linux console.
    #[must_use]
    pub fn on_unix(term: Option<&OsStr>) -> Self {
        if term.is_some_and(|term| term == "linux") {
            Self::ascii()
        } else {
            Self::unicode()
        }
    }

    /// Unicode in Windows Terminal, a terminal that names its program, and
    /// the `TERM`s of terminals that were made for it.
    #[must_use]
    pub fn on_windows(
        term: Option<&OsStr>,
        wt_session: Option<&OsStr>,
        term_program: Option<&OsStr>,
    ) -> Self {
        if wt_session.is_some()
            || term_program.is_some()
            || term.is_some_and(|term| term == "xterm-256color" || term == "alacritty")
        {
            Self::unicode()
        } else {
            Self::ascii()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use rstest::rstest;

    use crate::render::symbols::Symbols;

    #[rstest]
    #[case::linux_console(Some("linux"), Symbols::ascii())]
    #[case::xterm(Some("xterm-256color"), Symbols::unicode())]
    #[case::unset(None, Symbols::unicode())]
    fn unix_is_unicode_except_on_the_console(
        #[case] term: Option<&str>,
        #[case] symbols: Symbols,
    ) {
        assert_eq!(Symbols::on_unix(term.map(OsStr::new)), symbols);
    }

    #[rstest]
    #[case::windows_terminal(None, Some("1"), None, Symbols::unicode())]
    #[case::vscode(None, None, Some("vscode"), Symbols::unicode())]
    #[case::alacritty(Some("alacritty"), None, None, Symbols::unicode())]
    #[case::old_console(None, None, None, Symbols::ascii())]
    fn windows_is_unicode_in_modern_hosts(
        #[case] term: Option<&str>,
        #[case] wt_session: Option<&str>,
        #[case] program: Option<&str>,
        #[case] symbols: Symbols,
    ) {
        assert_eq!(
            Symbols::on_windows(
                term.map(OsStr::new),
                wt_session.map(OsStr::new),
                program.map(OsStr::new)
            ),
            symbols
        );
    }
}
