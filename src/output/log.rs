//! The plain lines of the rail: intro, log, note, outro and cancel.
use std::fmt::Display;

use crate::{
    core::{error::Result, session::Session, settings::OutputMode},
    render::draw::{self, NoteLine, Tone},
    terminal::Terminal,
};

impl<T: Terminal> Session<T> {
    /// Opens the rail with a title line; pass the bare title, it is padded and
    /// shown reversed as clack does.
    /// # Errors
    /// The terminal failed.
    pub fn intro(&self, title: impl Display) -> Result<()> {
        match self.mode() {
            OutputMode::Animated => self.write_live(&draw::clear_line())?,
            OutputMode::Accessible => {}
        }
        self.write(&draw::intro(self.skin(), &title.to_string()))
    }

    /// One log entry: a symbol for the tone, then the message on the rail. A
    /// message of several lines keeps its first line beside the symbol and puts
    /// the rest under it on the rail, as clack does.
    /// # Errors
    /// The terminal failed.
    pub fn log(&self, tone: Tone, message: impl Display) -> Result<()> {
        self.write(&draw::log(self.skin(), tone, &message.to_string()))
    }

    /// Prints `text` as is, with no rail and no style, and ends its last line
    /// so the next prompt or log entry starts clean.
    /// # Errors
    /// The terminal failed.
    pub fn print(&self, text: impl Display) -> Result<()> {
        let text = text.to_string();
        let end = if text.ends_with('\n') { "" } else { "\n" };
        self.write(&format!("{text}{end}"))
    }

    /// A box with a title and the lines inside.
    /// # Errors
    /// The terminal failed.
    pub fn note(&self, title: impl Display, lines: &[NoteLine]) -> Result<()> {
        self.write(&draw::boxed(
            self.skin(),
            &title.to_string(),
            lines,
            self.width(),
        ))
    }

    /// Closes the rail with a closing line. Any text that is [`Display`] will
    /// do, `format_args!` among them.
    ///
    /// ```no_run
    /// use clacky::prelude::*;
    /// # fn main() -> clacky::core::error::Result<()> {
    /// let ui = Session::detect()?;
    /// let (name, port) = ("my-app", 8080);
    /// ui.intro(name)?;
    /// ui.outro(format_args!("{name} on {port}"))
    /// # }
    /// ```
    /// # Errors
    /// The terminal failed.
    pub fn outro(&self, message: impl Display) -> Result<()> {
        self.write(&draw::outro(self.skin(), &message.to_string()))
    }

    /// Closes the rail with a red line.
    /// # Errors
    /// The terminal failed.
    pub fn cancel(&self, message: impl Display) -> Result<()> {
        self.cancel_with(message, std::iter::empty::<String>())
    }

    /// Like [`cancel`](Self::cancel), with one cause line under the message
    /// for each of `causes`.
    /// # Errors
    /// The terminal failed.
    pub fn cancel_with(
        &self,
        message: impl Display,
        causes: impl IntoIterator<Item = impl Display>,
    ) -> Result<()> {
        let causes: Vec<String> =
            causes.into_iter().map(|cause| cause.to_string()).collect();
        self.write(&draw::error_outro(
            self.skin(),
            &message.to_string(),
            &causes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core::{session::Session, settings::Settings},
        fake::{Script, screen},
        render::{color::ColorChoice, draw::Tone},
        terminal::TerminalKind,
    };

    fn session() -> Session<Script> {
        Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        )
    }

    #[test]
    fn text_is_anything_displayable() {
        let session = session();
        let port = 8080;
        let done = session
            .intro(format_args!(" app {port} "))
            .and_then(|()| session.log(Tone::Info, String::from("owned")))
            .and_then(|()| session.log(Tone::Info, "borrowed"))
            .and_then(|()| session.outro(format!("done on {port}")));
        assert!(done.is_ok());
        let shown = screen(&session.terminal().written());
        assert!(shown.contains("app 8080"), "{shown}");
        assert!(
            shown.contains("●  owned") && shown.contains("●  borrowed"),
            "{shown}"
        );
        assert!(shown.ends_with("└  done on 8080"), "{shown}");
    }

    #[test]
    fn intro_pads_a_bare_title_and_does_not_pad_twice() {
        let shown = |title: &str| {
            let session = session();
            assert!(session.intro(title).is_ok());
            screen(&session.terminal().written())
        };
        assert_eq!(shown("setup"), "┌   setup\n│");
        assert_eq!(shown(" setup "), shown("setup"));
    }

    #[test]
    fn print_writes_text_as_is_and_the_next_entry_starts_clean() {
        let session = session();
        let done = session
            .print("one\n  two")
            .and_then(|()| session.print("three\n"))
            .and_then(|()| session.log(Tone::Info, "after"));
        assert!(done.is_ok());
        assert_eq!(
            screen(&session.terminal().written()),
            "one\n  two\nthree\n●  after\n│"
        );
    }

    #[test]
    fn log_keeps_the_first_line_by_the_symbol_and_the_rest_on_the_rail() {
        let session = session();
        assert!(session.log(Tone::Success, "Saved\n~/a\n\nend").is_ok());
        assert_eq!(
            screen(&session.terminal().written()),
            "◆  Saved\n│  ~/a\n│\n│  end\n│"
        );
    }

    #[test]
    fn cancel_has_no_causes_and_cancel_with_lists_them() {
        let plain = session();
        assert!(plain.cancel(format_args!("no {}", "go")).is_ok());
        assert_eq!(screen(&plain.terminal().written()), "└  no go");
        let caused = session();
        assert!(caused.cancel_with("failed", ["one", "two"]).is_ok());
        assert_eq!(
            screen(&caused.terminal().written()),
            "└  failed\n   one\n   two"
        );
    }
}
