//! The plain lines of the rail: intro, log, note, outro and cancel.
use crate::{
    core::{error::Result, session::Session, settings::OutputMode},
    render::draw::{self, NoteLine, Tone},
    terminal::Terminal,
};

impl<T: Terminal> Session<T> {
    /// Opens the rail with a title line.
    /// # Errors
    /// The terminal failed.
    pub fn intro(&self, title: &str) -> Result<()> {
        match self.mode() {
            OutputMode::Animated => self.write_live(&draw::clear_line())?,
            OutputMode::Accessible => {}
        }
        self.write(&draw::intro(self.skin(), title))
    }

    /// One log entry: a symbol for the tone, then the message on the rail.
    /// # Errors
    /// The terminal failed.
    pub fn log(&self, tone: Tone, message: &str) -> Result<()> {
        self.write(&draw::log(self.skin(), tone, message))
    }

    /// A box with a title and the lines inside.
    /// # Errors
    /// The terminal failed.
    pub fn note(&self, title: &str, lines: &[NoteLine]) -> Result<()> {
        self.write(&draw::boxed(self.skin(), title, lines, self.width()))
    }

    /// # Errors
    /// The terminal failed, or the person stopped the form.
    pub fn outro(&self, message: &str) -> Result<()> {
        self.write(&draw::outro(self.skin(), message))
    }

    /// # Errors
    /// The terminal failed, or the person stopped the form.
    pub fn cancel(&self, message: &str, causes: &[String]) -> Result<()> {
        self.write(&draw::error_outro(self.skin(), message, causes))
    }
}
