//! Output that is not a question: log entries with a custom look, framed
//! boxes, streamed text, and the live spinner, progress bar, tasks and task
//! log.

pub mod boxed;
mod log;
pub mod progress;
pub mod spinner;
pub mod stream;
pub mod task_log;
pub mod tasks;

use crate::{
    core::{error::Result, session::Session},
    render::draw::{self, Tone},
    terminal::Terminal,
};

/// A log entry with a symbol or spacing of the caller's choosing; build it
/// with [`Session::log_entry`].
///
/// ```no_run
/// # use clacky::{core::session::Session, render::draw::Tone, terminal::Terminal};
/// # fn show<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// session
///     .log_entry(Tone::Message, "first line\nsecond line")
///     .symbol("→")
///     .spacing(0)
///     .show()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LogEntry<'s, T: Terminal> {
    session: &'s Session<T>,
    tone: Tone,
    message: String,
    symbol: Option<String>,
    spacing: usize,
}

impl<T: Terminal> Session<T> {
    /// Starts a log entry; the message may span lines, the rest sit on the
    /// rail.
    pub fn log_entry(&self, tone: Tone, message: impl Into<String>) -> LogEntry<'_, T> {
        LogEntry {
            session: self,
            tone,
            message: message.into(),
            symbol: None,
            spacing: 1,
        }
    }
}

impl<T: Terminal> LogEntry<'_, T> {
    /// Replaces the tone's symbol; shown as given, so paint it first if needed.
    #[must_use]
    pub fn symbol(self, symbol: impl Into<String>) -> Self {
        Self {
            symbol: Some(symbol.into()),
            ..self
        }
    }

    /// How many bare rail rows follow the entry; one by default. Clack puts
    /// them before the entry, here the previous element already ends on a rail
    /// row, so they go after.
    #[must_use]
    pub fn spacing(self, rows: usize) -> Self {
        Self {
            spacing: rows,
            ..self
        }
    }

    /// # Errors
    /// The terminal failed.
    pub fn show(self) -> Result<()> {
        let skin = self.session.skin();
        let symbol = self.symbol.unwrap_or_else(|| self.tone.symbol(skin));
        self.session
            .write(&draw::log_entry(skin, &symbol, &self.message, self.spacing))
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
    fn custom_symbol_and_no_spacing() {
        let session = session();
        let done = session
            .log_entry(Tone::Message, "one\n\ntwo")
            .symbol("→")
            .spacing(0)
            .show();
        assert!(done.is_ok());
        assert_eq!(screen(&session.terminal().written()), "→  one\n│\n│  two");
    }

    #[test]
    fn default_entry_matches_log() {
        let session = session();
        let done = session.log_entry(Tone::Info, "hi").spacing(2).show();
        assert!(done.is_ok());
        assert_eq!(screen(&session.terminal().written()), "●  hi\n│\n│");
    }
}
