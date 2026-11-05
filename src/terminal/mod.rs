//! The [`Terminal`] trait, [`Console`] over the real terminal, and key reading.

use std::io::{self, IsTerminal as _, Write as _};

use crate::{
    core::clock::Ticker,
    terminal::{
        abort::Abort,
        detect::{ColorHint, StreamKind, TerminalType},
    },
    values::line::Key,
};

pub mod abort;
#[cfg(any(windows, test))] mod console_keys;
pub mod detect;
#[cfg(unix)] mod keys;
pub mod signals;
pub mod tty;
#[cfg(windows)] mod windows;

const INTERRUPT_POLL: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// What a wait for a key ended with.
pub enum Press {
    /// The key that was pressed.
    Key(Key),
    /// The [`Abort`] fired first; no key was taken.
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// What a wait for a line ended with.
pub enum LineRead {
    /// The line, without its line ending.
    Line(String),
    /// The input ended; no line is left.
    End,
    /// The [`Abort`] fired first.
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// How the session can talk to the person.
pub enum TerminalKind {
    /// Keys and cursor movement.
    Live,
    /// Lines in and out, no cursor movement.
    Dumb,
    /// Output is piped: lines in, plain text out.
    Redirected,
}

/// Where a session reads keys and lines and writes its output.
pub trait Terminal {
    /// The guard of a quieted terminal; dropping it restores the terminal.
    type QuietTerminal: std::fmt::Debug;

    /// Quiets the terminal until the guard is dropped: no echo on a live one,
    /// so Ctrl-C prints no `^C`.
    fn quiet(&self) -> Self::QuietTerminal;

    /// How many Ctrl-C signals were caught since the program started.
    fn interrupts(&self) -> u64;

    /// Catches Ctrl-C, so that [`interrupts`](Self::interrupts) counts it,
    /// until the guard is dropped.
    fn catch_interrupts(&self) -> signals::Catching;

    /// How the session can talk to the person.
    fn kind(&self) -> TerminalKind;

    /// Whether output to this terminal may carry color.
    fn has_color(&self) -> bool;

    /// The width in columns; `None` when unknown.
    fn width(&self) -> Option<usize>;

    /// Writes `text` as is, escapes included, and flushes.
    ///
    /// # Errors
    /// The terminal could not be written.
    fn write(&self, text: &str) -> io::Result<()>;

    /// The next keystroke ([`TerminalKind::Live`] only), or [`Press::Aborted`]
    /// as soon as `abort` fires, also while the read is blocked on input.
    ///
    /// # Errors
    /// The terminal could not be read.
    fn keystroke(&self, abort: &Abort) -> io::Result<Press>;

    /// The next line of stdin without its line ending, or [`LineRead::Aborted`]
    /// as soon as `abort` fires, also while the read is blocked on input.
    ///
    /// # Errors
    /// The terminal could not be read.
    fn line(&self, abort: &Abort) -> io::Result<LineRead>;

    /// Runs `work` with the echo off and restores it however `work` ends.
    ///
    /// # Errors
    /// The terminal mode could not be changed.
    fn hidden<R>(&self, work: impl FnOnce(&Self) -> R) -> io::Result<R>;

    /// Drops unread input, so the rest of a paste reaches no question and no
    /// shell.
    fn discard_typeahead(&self);

    /// Resolves on the next Ctrl-C after the call; keep a
    /// [`catch_interrupts`](Self::catch_interrupts) guard while awaiting it.
    fn interrupt(&self) -> impl Future<Output = ()>;
}

#[derive(Debug)]
/// The real terminal: stdin and stderr of the process.
pub struct Console {
    kind: TerminalKind,
    color: bool,
}

impl Console {
    /// Checks which of the process's streams are terminals, and reads the color
    /// variables.
    #[must_use]
    pub fn detect(term_type: TerminalType) -> Self {
        #[cfg(windows)]
        tty::remember();
        let stderr = StreamKind::terminal_if(io::stderr().is_terminal());
        // Redraws need escapes whether or not there is colour.
        let ansi = ansi_ready();
        let kind = kind_of(
            StreamKind::terminal_if(io::stdin().is_terminal()),
            stderr,
            term_type,
            ansi,
        );
        let color = ColorHint::from_process().colored_on(stderr) && ansi;
        Self { kind, color }
    }
}

#[cfg(windows)]
fn ansi_ready() -> bool {
    windows::enable_ansi_on_stderr()
}

#[cfg(not(windows))]
const fn ansi_ready() -> bool {
    true
}

#[must_use]
const fn kind_of(
    stdin: StreamKind,
    stderr: StreamKind,
    term_type: TerminalType,
    ansi: bool,
) -> TerminalKind {
    match (stdin, stderr, term_type) {
        (StreamKind::Terminal, StreamKind::Terminal, TerminalType::Capable) if ansi => {
            TerminalKind::Live
        }
        (StreamKind::Terminal, _, _) => TerminalKind::Dumb,
        (StreamKind::Redirected, _, _) => TerminalKind::Redirected,
    }
}

impl Terminal for Console {
    type QuietTerminal = Option<tty::QuietedTerminal>;

    fn quiet(&self) -> Self::QuietTerminal {
        // Questions switch the keys quiet on top of this.
        match self.kind {
            TerminalKind::Live => tty::quiet(tty::TerminalQuiet::Echo).ok(),
            TerminalKind::Dumb | TerminalKind::Redirected => None,
        }
    }

    fn interrupts(&self) -> u64 {
        signals::interrupts()
    }

    fn catch_interrupts(&self) -> signals::Catching {
        signals::Catching::start()
    }

    fn kind(&self) -> TerminalKind {
        self.kind
    }

    fn has_color(&self) -> bool {
        self.color
    }

    fn width(&self) -> Option<usize> {
        columns()
    }

    fn write(&self, text: &str) -> io::Result<()> {
        signals::note_cursor(text);
        let mut stderr = io::stderr().lock();
        stderr
            .write_all(text.as_bytes())
            .and_then(|()| stderr.flush())
    }

    fn keystroke(&self, abort: &Abort) -> io::Result<Press> {
        if abort.ended() {
            return Ok(Press::Aborted);
        }
        tty::exclusive(|| read_keystroke(abort))
    }

    fn line(&self, abort: &Abort) -> io::Result<LineRead> {
        read_line(abort)
    }

    fn hidden<R>(&self, work: impl FnOnce(&Self) -> R) -> io::Result<R> {
        // Only live reads key by key; there Ctrl-C and Ctrl-U stay keys.
        let quiet = match self.kind {
            TerminalKind::Live => tty::TerminalQuiet::Keys,
            TerminalKind::Dumb | TerminalKind::Redirected => tty::TerminalQuiet::Echo,
        };
        tty::quiet(quiet).map(|quieted| {
            let out = work(self);
            drop(quieted);
            out
        })
    }

    fn discard_typeahead(&self) {
        tty::discard_typeahead();
    }

    fn interrupt(&self) -> impl Future<Output = ()> {
        let seen = signals::interrupts();
        async move {
            let poll = Ticker::every(INTERRUPT_POLL);
            while signals::interrupts() == seen {
                poll.tick().await;
            }
        }
    }
}

#[cfg(unix)]
fn columns() -> Option<usize> {
    tty::columns()
}

#[cfg(windows)]
fn columns() -> Option<usize> {
    windows::columns()
}

#[cfg(unix)]
fn read_line(abort: &Abort) -> io::Result<LineRead> {
    tty::next_line(abort)
}

#[cfg(windows)]
fn read_line(abort: &Abort) -> io::Result<LineRead> {
    windows::read_line(abort)
}

#[cfg(unix)]
fn read_keystroke(abort: &Abort) -> io::Result<Press> {
    keys::read(|wait| tty::next_byte(wait, abort))
        .map(|key| key.map_or(Press::Aborted, Press::Key))
}

#[cfg(windows)]
fn read_keystroke(abort: &Abort) -> io::Result<Press> {
    windows::read_keystroke(abort)
}

#[must_use]
fn char_key(c: char) -> Key {
    match c {
        '\u{1}' => Key::Home,
        '\u{2}' => Key::Left,
        '\u{3}' => Key::Interrupt,
        '\u{4}' => Key::EndOfInput,
        '\u{5}' => Key::End,
        '\u{6}' => Key::Right,
        '\u{b}' => Key::ClearAfter,
        '\u{15}' => Key::ClearBefore,
        '\u{17}' => Key::DeleteWord,
        '\u{8}' | '\u{7f}' => Key::Backspace,
        '\t' => Key::Tab,
        '\r' | '\n' => Key::Enter,
        '\u{1b}' => Key::Cancel,
        c => Key::Char(c),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::terminal::{
        TerminalKind,
        detect::{StreamKind, TerminalType},
        kind_of,
    };

    #[rstest]
    #[case::interactive(
        StreamKind::Terminal,
        StreamKind::Terminal,
        TerminalType::Capable,
        TerminalKind::Live
    )]
    #[case::stderr_redirected(
        StreamKind::Terminal,
        StreamKind::Redirected,
        TerminalType::Capable,
        TerminalKind::Dumb
    )]
    #[case::dumb_terminal(
        StreamKind::Terminal,
        StreamKind::Terminal,
        TerminalType::Dumb,
        TerminalKind::Dumb
    )]
    #[case::piped(
        StreamKind::Redirected,
        StreamKind::Terminal,
        TerminalType::Capable,
        TerminalKind::Redirected
    )]
    #[case::piped_everything(
        StreamKind::Redirected,
        StreamKind::Redirected,
        TerminalType::Capable,
        TerminalKind::Redirected
    )]
    fn streams_decide_the_kind(
        #[case] stdin: StreamKind,
        #[case] stderr: StreamKind,
        #[case] term_type: TerminalType,
        #[case] kind: TerminalKind,
    ) {
        assert_eq!(kind_of(stdin, stderr, term_type, true), kind);
    }

    #[test]
    fn a_console_that_reads_no_escapes_is_dumb() {
        assert_eq!(
            kind_of(
                StreamKind::Terminal,
                StreamKind::Terminal,
                TerminalType::Capable,
                false
            ),
            TerminalKind::Dumb
        );
    }
}
