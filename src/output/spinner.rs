//! The `spinner`: a turning frame and a message for work that takes a while.

use std::{
    fmt::Display,
    time::{Duration, Instant},
};

use crate::{
    core::{
        clock::{Ticker, race},
        error::{Error, Result},
        session::Session,
        settings::OutputMode,
    },
    render::{
        draw::{self, HIDE_CURSOR, SHOW_CURSOR},
        style::Skin,
        wrap::wrap as wrap_text,
    },
    terminal::{Terminal, TerminalKind, signals::Catching},
};

/// The time between two frames.
pub const DELAY: Duration = Duration::from_millis(80);
/// Frames per dot, and frames in one round of dots: three dots, then one more
/// frame with all three before it starts over.
const FRAMES_PER_DOT: u32 = 8;
const DOTS_ROUND: u32 = 33;
const MAX_DOTS: u32 = 3;
const SUFFIX_ROOM: usize = 12;
const GUTTER: usize = 3;

/// What follows the message while the spinner turns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Indicator {
    /// One to three dots, one more every eight frames.
    #[default]
    Dots,
    /// The time spent, like `[5s]` or `[1m 30s]`.
    Timer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Stop,
    Cancel,
    Error,
    /// Takes the spinner off the screen, rail line included.
    Wipe,
}

#[derive(Debug)]
struct Running<T: Terminal> {
    message: String,
    frame: u32,
    started: Instant,
    rows: Vec<String>,
    _catching: Catching,
    _quiet: T::QuietTerminal,
}

#[derive(Debug)]
enum Phase<T: Terminal> {
    Ready,
    Running(Running<T>),
    Finished,
}

#[derive(Debug, Clone, Copy)]
struct Look<'a> {
    skin: &'a Skin,
    width: Option<usize>,
    indicator: Indicator,
    output: OutputMode,
}

impl Look<'_> {
    fn running(self, message: &str, frame: u32, elapsed: Duration) -> Vec<String> {
        let turned = match self.output {
            OutputMode::Animated => frame,
            OutputMode::Accessible => 0,
        };
        let (theme, symbols) = (self.skin.theme(), self.skin.symbols());
        let at = usize::try_from(turned).unwrap_or_default() % symbols.spinner.len();
        let glyph = symbols.spinner.get(at).copied().unwrap_or_default();
        let room = self
            .width
            .map(|width| width.saturating_sub(GUTTER + SUFFIX_ROOM).max(1));
        let rail = format!("{}  ", self.skin.paint(theme.rail, symbols.bar));
        Self::compose(
            &format!("{}  ", self.skin.paint(theme.busy, glyph)),
            message.trim_end_matches('.'),
            &self.suffix(frame, elapsed),
            room.map(|room| (room, rail.as_str())),
        )
    }

    fn ending(self, ending: Ending, message: &str, elapsed: Duration) -> Vec<String> {
        let (theme, symbols) = (self.skin.theme(), self.skin.symbols());
        let symbol = match ending {
            Ending::Stop => self.skin.paint(theme.submitted, symbols.step_submit),
            Ending::Cancel => self.skin.paint(theme.cancelled, symbols.step_cancel),
            Ending::Error => self.skin.paint(theme.error, symbols.step_error),
            Ending::Wipe => return Vec::new(),
        };
        let timer = match self.indicator {
            Indicator::Dots => String::new(),
            Indicator::Timer => format!(" {}", timer(elapsed)),
        };
        Self::compose(&format!("{symbol}  "), message, &timer, None)
    }

    fn compose(
        lead: &str,
        message: &str,
        suffix: &str,
        wrap: Option<(usize, &str)>,
    ) -> Vec<String> {
        let rows: Vec<String> = message
            .split('\n')
            .flat_map(|line| match wrap {
                Some((room, _)) => wrap_text(line, room, ""),
                None => vec![line.to_owned()],
            })
            .collect();
        let total = rows.len();
        rows.into_iter()
            .enumerate()
            .map(|(at, text)| {
                let head = match (at, wrap) {
                    (0, _) => lead,
                    (_, Some((_, rail))) => rail,
                    (_, None) => "",
                };
                let tail = if at + 1 == total { suffix } else { "" };
                format!("{head}{text}{tail}")
            })
            .collect()
    }

    fn suffix(self, frame: u32, elapsed: Duration) -> String {
        match (self.indicator, self.output) {
            (Indicator::Dots, OutputMode::Animated) => {
                let dots = (frame % DOTS_ROUND / FRAMES_PER_DOT).min(MAX_DOTS);
                ".".repeat(usize::try_from(dots).unwrap_or_default())
            }
            (Indicator::Dots, OutputMode::Accessible) => "...".to_owned(),
            (Indicator::Timer, OutputMode::Animated) => {
                format!(" {}", timer(elapsed))
            }
            (Indicator::Timer, OutputMode::Accessible) => String::new(),
        }
    }
}

fn timer(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs / 60 {
        0 => format!("[{secs}s]"),
        mins => format!("[{mins}m {}s]", secs % 60),
    }
}

/// A turning frame with a message. Ctrl-C ends it with a red `Canceled` line
/// and [`Error::Interrupted`].
///
/// ```no_run
/// # use clacky::{core::session::Session, terminal::Terminal};
/// # fn work<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// let mut spinner = session.spinner();
/// spinner.start("Installing")?;
/// // ... between steps of the work:
/// spinner.tick()?;
/// spinner.stop("Installed")?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct Spinner<'s, T: Terminal> {
    session: &'s Session<T>,
    indicator: Indicator,
    output: OutputMode,
    phase: Phase<T>,
}

impl<T: Terminal> Session<T> {
    /// A spinner: animated on a live terminal, one line per change otherwise
    /// and when the session's output is accessible.
    #[must_use]
    pub fn spinner(&self) -> Spinner<'_, T> {
        let output = match (self.terminal().kind(), self.mode()) {
            (TerminalKind::Live, OutputMode::Animated) => OutputMode::Animated,
            (TerminalKind::Live, OutputMode::Accessible)
            | (TerminalKind::Dumb | TerminalKind::Redirected, _) => {
                OutputMode::Accessible
            }
        };
        Spinner {
            session: self,
            indicator: Indicator::Dots,
            output,
            phase: Phase::Ready,
        }
    }
}

impl<'s, T: Terminal> Spinner<'s, T> {
    /// What follows the message; dots by default.
    #[must_use]
    pub const fn indicator(mut self, indicator: Indicator) -> Self {
        self.indicator = indicator;
        self
    }

    /// Overrides the output the terminal and the session chose.
    #[must_use]
    pub const fn output(mut self, output: OutputMode) -> Self {
        self.output = output;
        self
    }

    pub(crate) const fn mode(&self) -> OutputMode {
        self.output
    }

    fn look(&self) -> Look<'s> {
        Look {
            skin: self.session.skin(),
            width: self.session.terminal().width(),
            indicator: self.indicator,
            output: self.output,
        }
    }

    /// Opens the rail and draws the first frame. Does nothing when started
    /// before.
    ///
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn start(&mut self, message: impl Display) -> Result<()> {
        match self.phase {
            Phase::Ready => {}
            Phase::Running(_) | Phase::Finished => return Ok(()),
        }
        if self.session.terminal().interrupts() != 0 {
            self.phase = Phase::Finished;
            return Err(Error::Interrupted);
        }
        let look = self.look();
        let mut running = Running {
            message: message.to_string(),
            frame: 0,
            started: Instant::now(),
            rows: Vec::new(),
            _catching: self.session.terminal().catch_interrupts(),
            _quiet: self.session.terminal().quiet(),
        };
        let hidden = match look.output {
            OutputMode::Animated => HIDE_CURSOR,
            OutputMode::Accessible => "",
        };
        let rail = look
            .skin
            .paint(look.skin.theme().rail, look.skin.symbols().bar);
        let opened = self.session.write(&format!("{hidden}{rail}\n"));
        let shown = opened.and_then(|()| Self::show(self.session, look, &mut running));
        self.phase = Phase::Running(running);
        self.session.spinner_started();
        shown
    }

    /// Changes the message and draws it.
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn message(&mut self, message: impl Display) -> Result<()> {
        self.check_interrupt()?;
        let look = self.look();
        let session = self.session;
        match &mut self.phase {
            Phase::Running(running) => {
                running.message = message.to_string();
                Self::show(session, look, running)
            }
            Phase::Ready | Phase::Finished => Ok(()),
        }
    }

    /// Turns the frame one step; call it every [`DELAY`]. A spinner that draws
    /// lines only counts the step.
    ///
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn tick(&mut self) -> Result<()> {
        self.check_interrupt()?;
        let look = self.look();
        let session = self.session;
        match &mut self.phase {
            Phase::Running(running) => {
                running.frame = running.frame.saturating_add(1);
                match look.output {
                    OutputMode::Animated => Self::show(session, look, running),
                    OutputMode::Accessible => Ok(()),
                }
            }
            Phase::Ready | Phase::Finished => Ok(()),
        }
    }

    /// Ends with a green `◇` line.
    /// # Errors
    /// The terminal failed, or Ctrl-C came before the end.
    pub fn stop(&mut self, message: impl Display) -> Result<()> {
        self.check_interrupt()?;
        self.end(Ending::Stop, &message.to_string())
    }

    /// Ends with a red `■` line.
    /// # Errors
    /// The terminal failed.
    pub fn cancel(&mut self, message: impl Display) -> Result<()> {
        self.end(Ending::Cancel, &message.to_string())
    }

    /// Ends with a red `▲` line.
    /// # Errors
    /// The terminal failed.
    pub fn error(&mut self, message: impl Display) -> Result<()> {
        self.end(Ending::Error, &message.to_string())
    }

    /// Ticks every [`DELAY`] until `work` is done; the spinner goes on running,
    /// to be ended by the caller.
    /// # Errors
    /// The terminal failed, or Ctrl-C came before `work` was done.
    pub async fn during<W>(&mut self, work: impl Future<Output = W>) -> Result<W> {
        self.during_ticked(work, Ticker::every(DELAY)).await
    }

    async fn during_ticked<W>(
        &mut self,
        work: impl Future<Output = W>,
        ticker: Ticker,
    ) -> Result<W> {
        let mut work = std::pin::pin!(work);
        loop {
            match race(&mut work, ticker.tick()).await {
                Some(out) => return Ok(out),
                None => self.tick()?,
            }
        }
    }

    /// Takes the spinner off the screen, rail line included; a spinner that
    /// draws lines leaves them.
    /// # Errors
    /// The terminal failed.
    pub(crate) fn wipe(&mut self) -> Result<()> {
        self.end(Ending::Wipe, "")
    }

    fn show(
        session: &Session<T>,
        look: Look<'_>,
        running: &mut Running<T>,
    ) -> Result<()> {
        let rows =
            look.running(&running.message, running.frame, running.started.elapsed());
        match look.output {
            OutputMode::Animated => {
                let up = draw::screen_rows(&running.rows, look.width);
                session.write(&draw::repaint(up, &rows))?;
                running.rows = rows;
                Ok(())
            }
            OutputMode::Accessible => session.write(&draw::lines_of(&rows)),
        }
    }

    fn end(&mut self, ending: Ending, message: &str) -> Result<()> {
        let look = self.look();
        match std::mem::replace(&mut self.phase, Phase::Finished) {
            Phase::Running(running) => {
                self.session.spinner_ended();
                let rows = look.ending(ending, message, running.started.elapsed());
                let up = draw::screen_rows(&running.rows, look.width)
                    + usize::from(ending == Ending::Wipe);
                match look.output {
                    OutputMode::Animated => self
                        .session
                        .write(&format!("{}{SHOW_CURSOR}", draw::repaint(up, &rows))),
                    OutputMode::Accessible => {
                        self.session.write(&draw::lines_of(&rows))
                    }
                }
            }
            other @ (Phase::Ready | Phase::Finished) => {
                self.phase = other;
                Ok(())
            }
        }
    }

    fn check_interrupt(&mut self) -> Result<()> {
        if self.session.terminal().interrupts() == 0 {
            return Ok(());
        }
        if matches!(self.phase, Phase::Running(_)) {
            let cancelled = self.session.skin().messages().cancelled.to_string();
            self.end(Ending::Cancel, &cancelled)?;
        }
        Err(Error::Interrupted)
    }
}

impl<T: Terminal> Drop for Spinner<'_, T> {
    fn drop(&mut self) {
        if matches!(self.phase, Phase::Running(_)) {
            self.session.spinner_ended();
            if self.output == OutputMode::Animated {
                self.session.write(SHOW_CURSOR).unwrap_or_default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::{
        cell::Cell,
        io,
        time::{Duration, Instant},
    };

    use rstest::rstest;

    use crate::{
        core::{
            error::{Error, Result},
            session::Session,
            settings::{OutputMode, Settings},
        },
        fake::{Script, Switch, screen},
        output::spinner::{Indicator, Look, Phase, Spinner, timer},
        render::{color::ColorChoice, messages::Messages, style::Skin},
        terminal::{
            LineRead,
            Press,
            Terminal,
            TerminalKind,
            abort::Abort,
            signals::Catching,
        },
    };

    fn session(kind: TerminalKind) -> Session<Script> {
        Session::new(Script::new(kind, [], []), Settings::new(ColorChoice::Plain))
    }

    static PLAIN: Skin = Skin::new(ColorChoice::Plain);

    fn look(indicator: Indicator, output: OutputMode) -> Look<'static> {
        Look {
            skin: &PLAIN,
            width: Some(80),
            indicator,
            output,
        }
    }

    #[rstest]
    #[case::first(0, "")]
    #[case::second(8, ".")]
    #[case::third(16, "..")]
    #[case::fourth(24, "...")]
    #[case::last_of_round(32, "...")]
    #[case::starts_over(33, "")]
    fn dots_grow_one_per_eight_frames(#[case] frame: u32, #[case] dots: &str) {
        let look = look(Indicator::Dots, OutputMode::Animated);
        assert_eq!(look.suffix(frame, Duration::ZERO), dots);
    }

    #[rstest]
    #[case::seconds(Duration::from_secs(5), "[5s]")]
    #[case::minute(Duration::from_secs(60), "[1m 0s]")]
    #[case::minutes(Duration::from_secs(90), "[1m 30s]")]
    fn timer_shows_seconds_then_minutes(
        #[case] elapsed: Duration,
        #[case] shown: &str,
    ) {
        assert_eq!(timer(elapsed), shown);
    }

    #[test]
    fn a_running_frame_turns_and_the_end_replaces_it() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        spinner.start("Loading...")?;
        spinner.tick()?;
        let mid = screen(&session.terminal().written());
        assert_eq!(mid, "│\n◐  Loading");
        spinner.message("Almost")?;
        spinner.stop("Done")?;
        let raw = session.terminal().written();
        assert!(raw.starts_with("\x1b[?25l│\n"), "{raw:?}");
        assert!(raw.ends_with("\x1b[?25h"), "{raw:?}");
        assert!(raw.contains("◒  Loading"), "{raw:?}");
        assert_eq!(screen(&raw), "│\n◇  Done");
        Ok(())
    }

    fn started_ago<T: Terminal>(spinner: &mut Spinner<'_, T>, ago: Duration) {
        if let Phase::Running(running) = &mut spinner.phase {
            running.started =
                Instant::now().checked_sub(ago).unwrap_or_else(Instant::now);
        }
    }

    #[test]
    fn the_timer_counts_time_not_ticks() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner().indicator(Indicator::Timer);
        spinner.start("Waiting")?;
        started_ago(&mut spinner, Duration::from_secs(20));
        spinner.tick()?;
        assert_eq!(screen(&session.terminal().written()), "│\n◐  Waiting [20s]");
        started_ago(&mut spinner, Duration::from_secs(60));
        spinner.stop("Waited")?;
        assert_eq!(
            screen(&session.terminal().written()),
            "│\n◇  Waited [1m 0s]"
        );
        Ok(())
    }

    #[test]
    fn a_redraw_erases_the_old_rows_at_the_new_width() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        spinner.start("x".repeat(60))?;
        session.terminal().resize(30);
        spinner.tick()?;
        let raw = session.terminal().written();
        assert!(raw.contains("\x1b[3A"), "{raw:?}");
        Ok(())
    }

    #[rstest]
    #[case::cancel(false, "■  Nope")]
    #[case::error(true, "▲  Nope")]
    fn cancel_and_error_end_with_their_symbol(
        #[case] error: bool,
        #[case] line: &str,
    ) -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        spinner.start("Trying")?;
        if error {
            spinner.error("Nope")?;
        } else {
            spinner.cancel("Nope")?;
        }
        assert_eq!(screen(&session.terminal().written()), format!("│\n{line}"));
        Ok(())
    }

    #[rstest]
    #[case::piped(TerminalKind::Redirected)]
    #[case::dumb(TerminalKind::Dumb)]
    fn lines_are_one_per_change_and_nothing_redraws(
        #[case] kind: TerminalKind,
    ) -> Result<()> {
        let session = session(kind);
        let mut spinner = session.spinner();
        spinner.start("Loading...")?;
        spinner.tick()?;
        spinner.tick()?;
        spinner.message("Linking")?;
        spinner.stop("Done")?;
        assert_eq!(
            session.terminal().written(),
            "│\n◒  Loading...\n◒  Linking...\n◇  Done\n"
        );
        Ok(())
    }

    #[test]
    fn lines_can_be_asked_for_on_a_live_terminal() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner().output(OutputMode::Accessible);
        spinner.start("Loading")?;
        spinner.stop("Done")?;
        assert_eq!(session.terminal().written(), "│\n◒  Loading...\n◇  Done\n");
        Ok(())
    }

    #[test]
    fn a_long_message_wraps_and_the_redraw_erases_every_row() -> Result<()> {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        );
        let mut spinner = session.spinner();
        spinner.start("x".repeat(200))?;
        let shown = screen(&session.terminal().written());
        let rows: Vec<&str> = shown.lines().skip(1).collect();
        assert_eq!(rows.len(), 4, "{shown}");
        assert!(
            rows.iter().all(|row| row.chars().count() <= 80 - 12),
            "{shown}"
        );
        spinner.stop("Done")?;
        let raw = session.terminal().written();
        assert!(raw.contains("\x1b[4A"), "{raw:?}");
        assert_eq!(screen(&raw), "│\n◇  Done");
        Ok(())
    }

    #[derive(Debug)]
    struct Pressed {
        inner: Script,
        count: Cell<u64>,
    }

    impl Terminal for Pressed {
        type QuietTerminal = ();

        fn quiet(&self) {}

        fn interrupts(&self) -> u64 {
            self.count.get()
        }

        fn catch_interrupts(&self) -> Catching {
            Catching::nothing()
        }

        fn kind(&self) -> TerminalKind {
            self.inner.kind()
        }

        fn has_color(&self) -> bool {
            false
        }

        fn width(&self) -> Option<usize> {
            self.inner.width()
        }

        fn write(&self, text: &str) -> io::Result<()> {
            self.inner.write(text)
        }

        fn keystroke(&self, abort: &Abort) -> io::Result<Press> {
            self.inner.keystroke(abort)
        }

        fn line(&self, abort: &Abort) -> io::Result<LineRead> {
            self.inner.line(abort)
        }

        fn hidden<R>(&self, work: impl FnOnce(&Self) -> R) -> io::Result<R> {
            Ok(work(self))
        }

        fn discard_typeahead(&self) {}

        fn interrupt(&self) -> impl Future<Output = ()> {
            std::future::pending()
        }
    }

    #[test]
    fn ctrl_c_ends_a_running_spinner_with_the_red_frame() {
        let session = Session::new(
            Pressed {
                inner: Script::new(TerminalKind::Live, [], []),
                count: Cell::new(0),
            },
            Settings::new(ColorChoice::Plain),
        );
        let mut spinner = session.spinner();
        let started = spinner.start("Working");
        session.terminal().count.set(1);
        let ticked = spinner.tick();
        let stopped = spinner.stop("Done");
        let raw = session.terminal().inner.written();
        assert!(started.is_ok());
        assert!(matches!(ticked, Err(Error::Interrupted)), "{ticked:?}");
        assert!(matches!(stopped, Err(Error::Interrupted)), "{stopped:?}");
        assert_eq!(screen(&raw), "│\n■  Canceled");
        assert!(raw.ends_with("\x1b[?25h"), "{raw:?}");
    }

    #[test]
    fn other_messages_word_the_interrupted_spinner() {
        let session = Session::new(
            Pressed {
                inner: Script::new(TerminalKind::Live, [], []),
                count: Cell::new(0),
            },
            Settings::new(ColorChoice::Plain),
        )
        .messages(Messages::english().cancelled("Abgebrochen"));
        let mut spinner = session.spinner();
        let started = spinner.start("Working");
        session.terminal().count.set(1);
        let ticked = spinner.tick();
        assert!(started.is_ok());
        assert!(matches!(ticked, Err(Error::Interrupted)), "{ticked:?}");
        let raw = session.terminal().inner.written();
        assert_eq!(screen(&raw), "│\n■  Abgebrochen");
    }

    #[test]
    fn an_accessible_session_makes_the_spinner_print_lines() -> Result<()> {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        )
        .output(OutputMode::Accessible);
        let mut spinner = session.spinner();
        spinner.start("Loading")?;
        spinner.tick()?;
        spinner.stop("Done")?;
        assert_eq!(session.terminal().written(), "│\n◒  Loading...\n◇  Done\n");
        Ok(())
    }

    #[test]
    fn stop_after_ctrl_c_is_not_a_success() {
        let session = Session::new(
            Pressed {
                inner: Script::new(TerminalKind::Live, [], []),
                count: Cell::new(0),
            },
            Settings::new(ColorChoice::Plain),
        );
        let mut spinner = session.spinner();
        let started = spinner.start("Working");
        session.terminal().count.set(1);
        let stopped = spinner.stop("Done");
        assert!(started.is_ok());
        assert!(matches!(stopped, Err(Error::Interrupted)), "{stopped:?}");
        let shown = screen(&session.terminal().inner.written());
        assert!(shown.ends_with("■  Canceled"), "{shown}");
    }

    #[test]
    fn ctrl_c_before_the_start_draws_nothing() {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []).interrupted_before(),
            Settings::new(ColorChoice::Plain),
        );
        let started = session.spinner().start("Working");
        assert!(matches!(started, Err(Error::Interrupted)), "{started:?}");
        assert_eq!(session.terminal().written(), "");
    }

    #[test]
    fn a_spinner_keeps_the_terminal_quiet_while_it_runs() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        assert_eq!(session.terminal().switches(), []);
        spinner.start("Working")?;
        assert_eq!(session.terminal().switches(), [Switch::Quieted]);
        spinner.stop("Done")?;
        assert_eq!(
            session.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        Ok(())
    }

    #[test]
    fn a_dropped_spinner_puts_the_terminal_back() -> Result<()> {
        let session = session(TerminalKind::Live);
        session.spinner().start("Working")?;
        assert_eq!(
            session.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        Ok(())
    }

    #[test]
    fn a_wiped_spinner_leaves_no_trace() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        spinner.start("Working")?;
        spinner.tick()?;
        spinner.wipe()?;
        let raw = session.terminal().written();
        assert_eq!(screen(&raw), "");
        assert!(raw.ends_with("\x1b[?25h"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn a_dropped_spinner_gives_the_cursor_back() -> Result<()> {
        let session = session(TerminalKind::Live);
        session.spinner().start("Working")?;
        let raw = session.terminal().written();
        assert!(raw.ends_with("\x1b[?25h"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn during_ticks_on_the_clock_until_the_work_is_done() -> Result<()> {
        use std::{
            future::{Future as _, poll_fn},
            pin::pin,
            task::{Context, Poll, Waker},
        };

        use crate::core::clock::Ticker;

        let session = session(TerminalKind::Live);
        let mut spinner = session.spinner();
        spinner.start("Waiting")?;
        let (ticker, pulse) = Ticker::manual();
        let done = Cell::new(false);
        let work = poll_fn(|_| {
            if done.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        });
        let finished = {
            let mut during = pin!(spinner.during_ticked(work, ticker));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(during.as_mut().poll(&mut cx).is_pending());
            for _ in 0..2 {
                pulse.fire();
                assert!(during.as_mut().poll(&mut cx).is_pending());
            }
            done.set(true);
            during.as_mut().poll(&mut cx)
        };
        let Poll::Ready(finished) = finished else {
            return Err(Error::Cancelled);
        };
        finished?;
        spinner.stop("Waited")?;
        let raw = session.terminal().written();
        assert!(
            raw.contains("◐  Waiting") && raw.contains("◓  Waiting"),
            "{raw:?}"
        );
        assert!(!raw.contains("◑"), "{raw:?}");
        assert_eq!(screen(&raw), "│\n◇  Waited");
        Ok(())
    }
}
