//! The [`Session`]: one terminal and one `Settings`, through which prompts and
//! log lines are drawn on the rail.

#[cfg(feature = "tokio")] use std::num::NonZeroU8;
use std::{
    fmt::Write as _,
    mem,
    ops::ControlFlow,
    sync::{
        Mutex,
        PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

use zeroize::Zeroizing;

use crate::{
    core::{
        ask::{self, Ask, Frame, Next, Phase, Prefill, Redirected, WhenSingle},
        error::{Error, Result},
        keys::KeyMap,
        settings::{Defaults, OutputMode, Settings},
    },
    render::{
        color::{ColorChoice, measure_text_width},
        draw::{self, Echo},
        messages::Messages,
        style::{ColorDepth, Skin, Theme},
        symbols::{Guide, Symbols},
    },
    terminal::{
        Console,
        LineRead,
        Press,
        Terminal,
        TerminalKind,
        abort::Abort,
        detect::TerminalType,
    },
    values::line::Cancellation,
};

/// Shows the cursor again when dropped, also while a panic unwinds.
struct CursorHidden<'a, T: Terminal>(&'a Session<T>);

impl<T: Terminal> Drop for CursorHidden<'_, T> {
    fn drop(&mut self) {
        self.0.write_animated(draw::SHOW_CURSOR).unwrap_or_default();
    }
}

const CLEAR_BELOW: &str = "\x1b[J";
const WIPE_LINE: &str = "\r\x1b[2K";

fn csi(out: &mut String, count: usize, last: char) {
    write!(out, "\x1b[{count}{last}").unwrap_or_default();
}

struct Pen {
    at: usize,
    known: usize,
}

impl Pen {
    /// Lines past the last known one are made with newlines, because a
    /// cursor-down escape stops at the bottom of the screen.
    fn go_to(&mut self, out: &mut String, line: usize) {
        if line < self.at {
            csi(out, self.at - line, 'A');
        } else if line > self.at {
            let by_arrow = line.min(self.known) - self.at;
            if by_arrow > 0 {
                csi(out, by_arrow, 'B');
            }
            out.extend(std::iter::repeat_n(
                '\n',
                line.saturating_sub(self.known.max(self.at)),
            ));
            self.known = self.known.max(line);
        }
        self.at = line;
    }
}

#[derive(Debug)]
struct Painter {
    /// The rows on the screen; the cursor is on the line below them.
    shown: Vec<String>,
    width: Option<usize>,
    frame: Frame,
    out: String,
    mode: OutputMode,
}

impl Painter {
    fn new(skin: Skin, mode: OutputMode) -> Self {
        Self {
            shown: Vec::new(),
            width: None,
            frame: Frame::skinned(skin, None),
            out: String::new(),
            mode,
        }
    }

    fn paint(&mut self, width: Option<usize>) -> &str {
        self.out.clear();
        if let (OutputMode::Animated, Some(width)) = (self.mode, width) {
            self.fit(width);
        }
        let next = self.frame.rows();
        if self.mode == OutputMode::Accessible {
            Self::paint_lines(&mut self.out, &self.shown, next);
        } else if self.shown.is_empty() || self.width != width {
            self.out.push('\r');
            if !self.shown.is_empty() {
                // The old rows have reflowed to the new width.
                csi(&mut self.out, draw::screen_rows(&self.shown, width), 'A');
            }
            self.out.push_str(CLEAR_BELOW);
            for row in next {
                self.out.push_str(row);
                self.out.push('\n');
            }
        } else {
            Self::paint_changes(&mut self.out, &self.shown, next);
        }
        self.width = width;
        mem::swap(&mut self.shown, self.frame.rows_mut());
        &self.out
    }

    /// A row wider than the screen wraps, which would throw off the count of
    /// rows to move up on repaint.
    fn fit(&mut self, width: usize) {
        let ellipsis = self.frame.skin().symbols().ellipsis;
        for row in self.frame.rows_mut() {
            if measure_text_width(row) > width {
                *row = draw::clip_row(row, width, ellipsis);
            }
        }
    }

    fn paint_lines(out: &mut String, shown: &[String], next: &[String]) {
        for (line, row) in next.iter().enumerate() {
            if shown.get(line) != Some(row) {
                out.push_str(row);
                out.push('\n');
            }
        }
    }

    fn paint_changes(out: &mut String, shown: &[String], next: &[String]) {
        let mut pen = Pen {
            at: shown.len(),
            known: shown.len(),
        };
        for (line, row) in next.iter().enumerate() {
            if shown.get(line) != Some(row) {
                pen.go_to(out, line);
                out.push_str(WIPE_LINE);
                out.push_str(row);
            }
        }
        if out.is_empty() && shown.len() == next.len() {
            return;
        }
        pen.go_to(out, next.len());
        out.push('\r');
        if next.len() < shown.len() {
            out.push_str(CLEAR_BELOW);
        }
    }

    fn leave(&mut self, width: Option<usize>, left: &str) -> &str {
        self.out.clear();
        if self.mode == OutputMode::Animated {
            self.out.push('\r');
            if !self.shown.is_empty() {
                csi(&mut self.out, draw::screen_rows(&self.shown, width), 'A');
            }
            self.out.push_str(CLEAR_BELOW);
        }
        self.out.push_str(left);
        self.shown.clear();
        &self.out
    }
}

pub(crate) struct Scoped<'a, T: Terminal> {
    session: &'a Session<T>,
    before: Abort,
}

impl<T: Terminal> Drop for Scoped<'_, T> {
    fn drop(&mut self) {
        *self.session.scoped() = mem::take(&mut self.before);
    }
}

/// Owns the terminal and the settings; every prompt and log line goes through
/// it.
#[derive(Debug)]
pub struct Session<T: Terminal> {
    terminal: T,
    settings: Settings,
    spinners: AtomicUsize,
    abort: Abort,
    scope: Mutex<Abort>,
    /// The warning the next question opens under; set by `ask_until` only.
    warning: Mutex<Option<String>>,
}

impl Session<Console> {
    /// A session on the real terminal; the only place the library reads the
    /// environment (`TERM`, `COLORTERM`, `NO_COLOR`, `CLICOLOR`,
    /// `CLICOLOR_FORCE`).
    ///
    /// # Errors
    /// The terminal could not be set up.
    pub fn detect() -> Result<Self> {
        let console = Console::detect(TerminalType::from_process());
        let color = ColorChoice::colored_if(console.has_color());
        let skin = Skin::new(color)
            .with_depth(ColorDepth::from_process())
            .with_symbols(Symbols::from_process());
        let settings = Settings {
            skin,
            output: OutputMode::from_process(),
            ..Settings::new(color)
        };
        Ok(Self::new(console, settings))
    }
}

impl<T: Terminal> Session<T> {
    /// A session on `term`.
    pub fn new(term: T, settings: Settings) -> Self {
        Self {
            settings,
            spinners: AtomicUsize::new(0),
            abort: Abort::default(),
            scope: Mutex::new(Abort::default()),
            warning: Mutex::new(None),
            terminal: term,
        }
    }

    /// Ends every question the session asks, running or later, when `abort`
    /// fires; the question fails with [`Error::Aborted`], and a second call
    /// replaces the first.
    #[must_use]
    pub fn abort(mut self, abort: Abort) -> Self {
        self.abort = abort;
        self
    }

    fn scoped(&self) -> std::sync::MutexGuard<'_, Abort> {
        self.scope.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn abort_now(&self) -> Abort {
        self.abort.both(&self.scoped())
    }

    pub(crate) fn under(&self, abort: &Abort) -> Scoped<'_, T> {
        let mut scoped = self.scoped();
        let both = scoped.both(abort);
        let before = mem::replace(&mut *scoped, both);
        Scoped {
            session: self,
            before,
        }
    }

    /// Sets whether questions are asked or answered by their defaults; see
    /// [`Defaults`].
    #[must_use]
    pub fn defaults(self, defaults: Defaults) -> Self {
        Self {
            settings: self.settings.defaults(defaults),
            ..self
        }
    }

    pub(crate) fn spinner_started(&self) {
        self.spinners.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn spinner_ended(&self) {
        // `fetch_update` is deprecated on newer Rust and `try_update` is past
        // the MSRV.
        let mut count = self.spinners.load(Ordering::Relaxed);
        while let Some(less) = count.checked_sub(1) {
            match self.spinners.compare_exchange_weak(
                count,
                less,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(now) => count = now,
            }
        }
    }

    fn not_aborted(&self) -> Result<()> {
        if self.abort_now().ended() {
            Err(Error::Aborted)
        } else {
            Ok(())
        }
    }

    fn not_busy(&self) -> Result<()> {
        if self.spinners.load(Ordering::Relaxed) == 0 {
            Ok(())
        } else {
            Err(Error::Busy)
        }
    }

    pub(crate) fn width(&self) -> Option<usize> {
        self.terminal.width()
    }

    pub(crate) const fn skin(&self) -> &Skin {
        &self.settings.skin
    }

    /// Paints with `theme`; without color nothing is painted.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.settings.skin = self.settings.skin.with_theme(theme);
        self
    }

    /// Draws with `symbols` instead of the ones the terminal chose.
    #[must_use]
    pub fn symbols(mut self, symbols: Symbols) -> Self {
        self.settings.skin = self.settings.skin.with_symbols(symbols);
        self
    }

    /// Uses `messages` for the built-in texts.
    #[must_use]
    pub fn messages(mut self, messages: Messages) -> Self {
        self.settings.skin = self.settings.skin.with_messages(messages);
        self
    }

    /// Reads keys through `keys`.
    #[must_use]
    pub fn keys(mut self, keys: KeyMap) -> Self {
        self.settings.keys = keys;
        self
    }

    /// Leaves out the rail down the left and the bars of the intro and outro.
    #[doc(alias = "withGuide")]
    #[must_use]
    pub fn without_rail(mut self) -> Self {
        self.settings.skin = self.settings.skin.with_guide(Guide::Hidden);
        self
    }

    /// Sets whether the session redraws in place or writes plain lines.
    #[must_use]
    pub const fn output(mut self, output: OutputMode) -> Self {
        self.settings.output = output;
        self
    }

    pub(crate) const fn mode(&self) -> OutputMode {
        self.settings.output
    }

    /// The terminal the session talks to.
    #[must_use]
    pub const fn terminal(&self) -> &T {
        &self.terminal
    }

    /// Asks the question `make` builds and runs `attempt` on the answer while
    /// `label` shows a spinner; a failed attempt is retried up to `tries` times
    /// when `retry` gives a warning, and a redirected input is asked once. The
    /// question may end in [`Ask::map`], so `attempt` gets the mapped answer.
    ///
    /// # Errors
    /// The terminal failed, or the person stopped the question. When the tries
    /// run out, the last attempt error is in the inner `Result`.
    #[cfg(feature = "tokio")]
    pub async fn ask_until<Q: Ask, A, F>(
        &self,
        make: impl Fn() -> Q,
        label: &str,
        attempt: impl AsyncFn(&Q::Output) -> std::result::Result<A, F>,
        retry: impl Fn(&F) -> Option<String>,
        tries: NonZeroU8,
    ) -> Result<std::result::Result<A, F>> {
        self.try_until(&make, label, &attempt, &retry, (tries.get(), None))
            .await
    }

    #[cfg(feature = "tokio")]
    async fn try_until<Q: Ask, A, F>(
        &self,
        make: &impl Fn() -> Q,
        label: &str,
        attempt: &impl AsyncFn(&Q::Output) -> std::result::Result<A, F>,
        retry: &impl Fn(&F) -> Option<String>,
        (tries, warning): (u8, Option<&str>),
    ) -> Result<std::result::Result<A, F>> {
        *self.warning.lock().unwrap_or_else(PoisonError::into_inner) =
            warning.map(str::to_owned);
        let asked = make().run(self);
        // A question that never reached a prompt leaves its warning behind.
        self.warning
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let answer = asked?;
        let outcome = self.wait(label, attempt(&answer)).await?;
        let again =
            outcome.as_ref().err().and_then(retry).filter(|_| {
                tries > 1 && self.terminal.kind() != TerminalKind::Redirected
            });
        match again {
            None => Ok(outcome),
            Some(reason) => {
                Box::pin(self.try_until(
                    make,
                    label,
                    attempt,
                    retry,
                    (tries - 1, Some(&reason)),
                ))
                .await
            }
        }
    }

    /// Runs a question, or a tuple of questions in order, and returns the
    /// answers.
    ///
    /// # Errors
    /// The terminal failed, or the person stopped the question.
    pub fn ask<A: Ask>(&self, question: A) -> Result<A::Output> {
        question.run(self)
    }

    pub(crate) fn drive<P: ask::Prompt>(&self, prompt: P) -> Result<P::Output> {
        let warning = self
            .warning
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.drive_warned(prompt, warning.as_deref())
    }

    fn drive_warned<P: ask::Prompt>(
        &self,
        prompt: P,
        warning: Option<&str>,
    ) -> Result<P::Output> {
        self.not_busy()?;
        let abort = self.abort_now();
        if abort.ended() {
            return Err(Error::Aborted);
        }
        let state = prompt.start()?;
        if let Some(taken) = self.settled(&prompt)? {
            self.answered(
                prompt.message(),
                &self.shown(&prompt, &taken, Phase::Submitted),
            )?;
            return prompt.finish(taken);
        }
        let _quiet = self.terminal.quiet();
        match self.terminal.kind() {
            TerminalKind::Live => self.drive_keys(&prompt, state, warning, &abort),
            TerminalKind::Dumb => self.drive_lines(&prompt, state, warning, &abort),
            TerminalKind::Redirected => match prompt.redirected() {
                Redirected::Line => self.drive_piped(&prompt, state, &abort),
                Redirected::NoInput => Err(no_input(&prompt)),
            },
        }
        .and_then(|state| prompt.finish(state))
    }

    /// The state goes through Enter, so a check on the answer still applies.
    fn settled<P: ask::Prompt>(&self, prompt: &P) -> Result<Option<P::State>> {
        let state = match (self.settings.defaults_mode(), prompt.sole()) {
            (Defaults::Ask, None | Some((WhenSingle::Ask, _))) => return Ok(None),
            (Defaults::Ask | Defaults::Take, Some((WhenSingle::Skip, state)))
            | (Defaults::Take, Some((WhenSingle::Ask, state))) => state,
            (Defaults::Take, None) => {
                prompt.by_default().ok_or_else(|| Error::NoDefault {
                    question: prompt.message().to_owned(),
                })?
            }
        };
        self.pressed_enter(prompt, state).map(Some)
    }

    fn pressed_enter<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
    ) -> Result<P::State> {
        match prompt.update(state, ask::Key::Enter) {
            Next::Submit(state) => Ok(state),
            Next::Refuse(_, reason) => Err(Error::Refused {
                question: prompt.message().to_owned(),
                reason: reason.text(self.skin().messages()),
            }),
            Next::Quit(_, quit) => Err(stopped_by(quit)),
            Next::Editing(_) => Err(no_input(prompt)),
        }
    }

    pub(crate) fn take_given<P: Prefill>(
        &self,
        prompt: &P,
        answer: P::Output,
    ) -> Result<P::Output> {
        self.not_busy()?;
        self.not_aborted()?;
        let messages = self.skin().messages();
        let shown =
            prompt
                .shown_for(&answer, messages)
                .ok_or_else(|| Error::Refused {
                    question: prompt.message().to_owned(),
                    reason: prompt.refusal_of(&answer, messages).text(messages),
                })?;
        let shown = draw::masked(prompt.echo(), self.skin().symbols(), &shown);
        self.answered(prompt.message(), &shown).map(|()| answer)
    }

    pub(crate) fn take_line<P: ask::Prompt>(
        &self,
        prompt: P,
        line: &str,
    ) -> Result<P::Output> {
        self.not_busy()?;
        self.not_aborted()?;
        let invalid = |reason: &str| Error::Refused {
            question: prompt.message().to_owned(),
            reason: reason.to_owned(),
        };
        let done = match self.typed_line(&prompt, prompt.start()?, line) {
            Ok(Typed::Done(done)) => done,
            Ok(Typed::Refused(reason)) => return Err(invalid(&reason)),
            Err(Error::Cancelled | Error::Interrupted) => {
                return Err(invalid(&self.skin().messages().not_an_answer));
            }
            Err(other) => return Err(other),
        };
        self.answered(
            prompt.message(),
            &self.shown(&prompt, &done, Phase::Submitted),
        )?;
        prompt.finish(done)
    }

    fn drive_keys<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
        warning: Option<&str>,
        abort: &Abort,
    ) -> Result<P::State> {
        let done = self
            .terminal
            .hidden(|_| {
                let _shown_again = CursorHidden(self);
                self.write_animated(draw::HIDE_CURSOR)
                    .and_then(|()| self.press_all(prompt, state, warning, abort))
            })
            .map_err(Error::Terminal)?;
        self.terminal.discard_typeahead();
        done
    }

    fn rows_of<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: &P::State,
        phase: Phase<'_>,
    ) -> Vec<String> {
        let mut frame = Frame::skinned(self.skin().clone(), self.terminal.width());
        prompt.view(state, phase, &mut frame);
        frame.rows().to_vec()
    }

    fn fill_active<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: &P::State,
        warning: Option<&str>,
        frame: &mut Frame,
    ) {
        let skin = self.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let (head, corner, phase) = warning.map_or_else(
            || {
                (
                    skin.paint(theme.active, symbols.step_active),
                    skin.paint(theme.active, symbols.bar_end),
                    Phase::Active,
                )
            },
            |why| {
                (
                    skin.paint(theme.warning, symbols.step_error),
                    format!(
                        "{}  {}",
                        skin.paint(theme.warning, symbols.bar_end),
                        skin.code_on(why, |part| skin.paint(theme.warning, part))
                    ),
                    Phase::Warned(why),
                )
            },
        );
        frame.row(format!("{head}  {}", prompt.message()));
        prompt.view(state, phase, frame);
        frame.row(corner);
    }

    fn shown<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: &P::State,
        phase: Phase<'_>,
    ) -> String {
        self.rows_of(prompt, state, phase).join("\n")
    }

    fn press_all<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
        warning: Option<&str>,
        abort: &Abort,
    ) -> Result<P::State> {
        let mut painter = Painter::new(self.skin().clone(), self.mode());
        self.painted(prompt, &mut painter, &state, warning)?;
        let flow = std::iter::repeat_with(|| self.terminal.keystroke(abort)).try_fold(
            state,
            |state, keystroke| match keystroke {
                Err(error) => ControlFlow::Break(Err(Error::Terminal(error))),
                Ok(Press::Aborted) => ControlFlow::Break(
                    self.cancelled(prompt, &mut painter, &state)
                        .and(Err(Error::Aborted)),
                ),
                Ok(Press::Key(keystroke)) => {
                    let key = ask::read_key(
                        prompt,
                        keystroke,
                        &self.settings.keys,
                        self.skin().messages(),
                    );
                    match prompt.update(state, key) {
                        Next::Editing(state) => {
                            self.redrawn(prompt, &mut painter, state, None)
                        }
                        Next::Refuse(state, why) => self.redrawn(
                            prompt,
                            &mut painter,
                            state,
                            Some(&why.text(self.skin().messages())),
                        ),
                        Next::Submit(state) => ControlFlow::Break(
                            self.write(painter.leave(
                                self.terminal.width(),
                                &draw::answered(
                                    self.skin(),
                                    prompt.message(),
                                    &self.shown(prompt, &state, Phase::Submitted),
                                ),
                            ))
                            .map(|()| state),
                        ),
                        Next::Quit(state, quit) => ControlFlow::Break(
                            self.cancelled(prompt, &mut painter, &state)
                                .and(Err(stopped_by(quit))),
                        ),
                    }
                }
            },
        );
        match flow {
            ControlFlow::Break(done) => done,
            // Keystrokes never run out; if they did, nothing was submitted.
            ControlFlow::Continue(_) => Err(Error::Cancelled),
        }
    }

    fn cancelled<P: ask::Prompt>(
        &self,
        prompt: &P,
        painter: &mut Painter,
        state: &P::State,
    ) -> Result<()> {
        self.write(painter.leave(
            self.terminal.width(),
            &draw::cancelled(
                self.skin(),
                prompt.message(),
                &self.shown(prompt, state, Phase::Cancelled),
            ),
        ))
    }

    /// One write per picture.
    fn painted<P: ask::Prompt>(
        &self,
        prompt: &P,
        painter: &mut Painter,
        state: &P::State,
        warning: Option<&str>,
    ) -> Result<()> {
        let width = self.terminal.width();
        painter.frame.reset(width);
        self.fill_active(prompt, state, warning, &mut painter.frame);
        match painter.paint(width) {
            "" => Ok(()),
            text => self.write(text),
        }
    }

    fn redrawn<P: ask::Prompt>(
        &self,
        prompt: &P,
        painter: &mut Painter,
        state: P::State,
        warning: Option<&str>,
    ) -> ControlFlow<Result<P::State>, P::State> {
        self.painted(prompt, painter, &state, warning).map_or_else(
            |error| ControlFlow::Break(Err(error)),
            |()| ControlFlow::Continue(state),
        )
    }

    /// A refused line is asked again under its warning.
    fn drive_lines<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
        warning: Option<&str>,
        abort: &Abort,
    ) -> Result<P::State> {
        let listing: String = prompt
            .listing(self.skin().messages())
            .iter()
            .flat_map(|row| [row.as_str(), "\n"])
            .collect();
        let symbols = self.skin().symbols();
        let warned = warning.map_or_else(String::new, |why| {
            format!("{}  {why}\n", symbols.step_error)
        });
        let question = format!(
            "{warned}{}  {}\n{listing}",
            symbols.step_active,
            prompt.message()
        );
        let line = self.read_line(prompt, &question, abort)?;
        match self.typed_line(prompt, state, &line)? {
            Typed::Done(done) => self
                .answered(
                    prompt.message(),
                    &self.shown(prompt, &done, Phase::Submitted),
                )
                .map(|()| done),
            Typed::Refused(why) => {
                self.drive_lines(prompt, prompt.start()?, Some(&why), abort)
            }
        }
    }

    /// A refused line is an error: there is nobody to ask again.
    fn drive_piped<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
        abort: &Abort,
    ) -> Result<P::State> {
        let line = match self.terminal.line(abort).map_err(Error::Terminal)? {
            LineRead::Line(line) => Zeroizing::new(line),
            LineRead::End => return Err(no_input(prompt)),
            LineRead::Aborted => return Err(Error::Aborted),
        };
        match self.typed_line(prompt, state, &line)? {
            Typed::Done(done) => self
                .answered(
                    prompt.message(),
                    &self.shown(prompt, &done, Phase::Submitted),
                )
                .map(|()| done),
            Typed::Refused(reason) => Err(Error::Refused {
                question: prompt.message().to_owned(),
                reason,
            }),
        }
    }

    fn read_line<P: ask::Prompt>(
        &self,
        prompt: &P,
        question: &str,
        abort: &Abort,
    ) -> Result<Zeroizing<String>> {
        let read = |term: &T| {
            term.write(question)
                .and_then(|()| term.line(abort))
                .inspect(|_| term.discard_typeahead())
        };
        let line = match prompt.echo() {
            Echo::Masked(_) => {
                self.terminal.hidden(read).map_err(Error::EchoNotHidden)?
            }
            Echo::Shown => read(&self.terminal),
        };
        match line.map_err(Error::Terminal)? {
            LineRead::Line(line) => Ok(Zeroizing::new(line)),
            LineRead::End => Err(Error::Cancelled),
            LineRead::Aborted => Err(Error::Aborted),
        }
    }

    fn typed_line<P: ask::Prompt>(
        &self,
        prompt: &P,
        state: P::State,
        line: &str,
    ) -> Result<Typed<P::State>> {
        if let Some(why) = prompt.misread(line, self.skin().messages()) {
            return Ok(Typed::Refused(why.text(self.skin().messages())));
        }
        let flow = prompt
            .typed(line, self.skin().messages())
            .into_iter()
            .try_fold(state, |state, key| match prompt.update(state, key) {
                Next::Editing(state) => ControlFlow::Continue(state),
                Next::Refuse(_, why) => ControlFlow::Break(Ok(Typed::Refused(
                    why.text(self.skin().messages()),
                ))),
                Next::Submit(state) => ControlFlow::Break(Ok(Typed::Done(state))),
                Next::Quit(_, quit) => ControlFlow::Break(Err(stopped_by(quit))),
            });
        match flow {
            ControlFlow::Break(done) => done,
            ControlFlow::Continue(_) => Err(Error::Cancelled),
        }
    }

    pub(crate) fn answered(&self, label: &str, value: &str) -> Result<()> {
        self.write(&draw::answered(self.skin(), label, value))
    }

    /// Awaits `work` under a spinner labelled `label`; without a live terminal
    /// nothing is drawn.
    ///
    /// # Errors
    /// The terminal failed, or Ctrl-C ended the wait.
    #[cfg(feature = "tokio")]
    pub async fn wait<W>(
        &self,
        label: &str,
        work: impl Future<Output = W>,
    ) -> Result<W> {
        // Catch before the line shows, so an early Ctrl-C reaches this wait.
        let _catching = self.terminal.catch_interrupts();
        let interrupt = self.terminal.interrupt();
        match self.terminal.kind() {
            TerminalKind::Live => {
                let mut spinner = self.spinner();
                spinner.start(label)?;
                let out = tokio::select! {
                    biased;
                    out = spinner.during(work) => out,
                    () = interrupt => {
                        spinner.cancel(self.skin().messages().cancelled.to_string())?;
                        Err(Error::Interrupted)
                    }
                }?;
                spinner.wipe()?;
                Ok(out)
            }
            TerminalKind::Dumb | TerminalKind::Redirected => {
                tokio::select! {
                    biased;
                    out = work => Ok(out),
                    () = interrupt => Err(Error::Interrupted),
                }
            }
        }
    }

    pub(crate) fn write_live(&self, text: &str) -> Result<()> {
        match self.terminal.kind() {
            TerminalKind::Live => self.write(text),
            TerminalKind::Dumb | TerminalKind::Redirected => Ok(()),
        }
    }

    pub(crate) fn write_animated(&self, text: &str) -> Result<()> {
        match self.mode() {
            OutputMode::Animated => self.write(text),
            OutputMode::Accessible => Ok(()),
        }
    }

    pub(crate) fn write(&self, text: &str) -> Result<()> {
        self.terminal.write(text).map_err(Error::Terminal)
    }
}

enum Typed<S> {
    Done(S),
    Refused(String),
}

pub(crate) fn no_input(prompt: &impl ask::Prompt) -> Error {
    Error::NoInput {
        question: prompt.message().to_owned(),
    }
}

pub(crate) fn stopped_by(quit: Cancellation) -> Error {
    match quit {
        Cancellation::Interrupt => Error::Interrupted,
        Cancellation::Cancel => Error::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::time::{Duration, Instant};

    use rstest::rstest;

    use crate::{
        core::{
            ask::{Ask as _, CancelAsNone as _, Peek, Prefill, Prompt as _},
            error::{Error, Result},
            keys::{Action, KeyMap},
            session::{Painter, Session},
            settings::{Defaults, OutputMode, Settings},
        },
        fake::{Script, Switch, screen, testing::bare},
        prompts::{
            confirm::{Reply, confirm},
            multiselect::multiselect,
            password::password,
            select::select,
            text::text as ask_text,
        },
        render::{
            color::ColorChoice,
            draw::{HIDE_CURSOR, NoteLine, NoteTone, SHOW_CURSOR, Tone},
            messages::Messages,
            style::{Color, ColorDepth, Skin, Style, Theme},
            symbols::Symbols,
        },
        terminal::{TerminalKind, abort::Abort},
        values::line::Key,
    };

    fn chars(text: &'static str) -> impl Iterator<Item = Key> {
        text.chars().map(Key::Char)
    }

    fn form(script: Script) -> Result<Session<Script>> {
        let session = bare(script);
        session.intro("acme login").map(|()| session)
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn waiting_line_is_drawn_and_removed_only_when_live() -> Result<()> {
        let live = form(Script::new(TerminalKind::Live, [], []))?;
        let piped = form(Script::new(TerminalKind::Redirected, [], []))?;
        let work = || async { 7 };
        assert_eq!(live.wait("Checking", work()).await?, 7);
        assert_eq!(piped.wait("Checking", work()).await?, 7);
        assert!(live.terminal().written().contains("◒  Checking"));
        assert!(!piped.terminal().written().contains('◒'));
        assert!(!screen(&live.terminal().written()).contains('◒'));
        Ok(())
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn ctrl_c_during_a_wait_closes_the_form() -> Result<()> {
        let form = form(Script::new(TerminalKind::Live, [], []).pressing_ctrl_c())?;
        let err = form
            .wait("Signing in", std::future::pending::<()>())
            .await
            .err();
        assert!(matches!(err, Some(Error::Interrupted)), "{err:?}");
        form.cancel("login cancelled")?;
        let shown = screen(&form.terminal().written());
        assert!(!shown.contains('◒'), "{shown}");
        assert!(shown.ends_with("└  login cancelled"), "{shown}");
        Ok(())
    }

    #[rstest]
    #[case::no('n', false)]
    #[case::yes('y', true)]
    fn the_letters_of_a_confirm_win_over_a_rebound_key(
        #[case] letter: char,
        #[case] answer: bool,
    ) -> Result<()> {
        let keys = KeyMap::clack()
            .bind(Key::Char('n'), Action::Down)
            .bind(Key::Char('y'), Action::Down);
        let script =
            Script::new(TerminalKind::Live, [Key::Char(letter), Key::Enter], []);
        let ui = bare(script).keys(keys);
        assert_eq!(ui.ask(confirm("Go?").default(Reply::No))?, answer);
        Ok(())
    }

    #[test]
    fn a_ctrl_c_counted_earlier_does_not_stop_a_new_question() -> Result<()> {
        let form = form(
            Script::new(TerminalKind::Live, chars("a@b.c").chain([Key::Enter]), [])
                .interrupted_before(),
        )?;
        assert_eq!(form.ask(ask_text("Email"))?, "a@b.c");
        Ok(())
    }

    #[rstest]
    #[case::live(TerminalKind::Live, "\r\x1b[2K┌")]
    #[case::piped(TerminalKind::Redirected, "┌")]
    fn intro_starts_on_a_clean_line(
        #[case] kind: TerminalKind,
        #[case] opening: &str,
    ) -> Result<()> {
        let form = form(Script::new(kind, [], []))?;
        let written = form.terminal().written();
        assert!(written.starts_with(opening), "{written:?}");
        assert_eq!(screen(&written).lines().next(), Some("┌   acme login"));
        Ok(())
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn finished_work_wins_over_ctrl_c() -> Result<()> {
        let form = form(Script::new(TerminalKind::Live, [], []).pressing_ctrl_c())?;
        let work = async { 7 };
        assert_eq!(form.wait("Checking", work).await?, 7);
        Ok(())
    }

    #[test]
    fn log_and_note_write_through_the_session() -> Result<()> {
        let session = bare(Script::new(TerminalKind::Live, [], []));
        session.log(Tone::Warn, "careful")?;
        session.note(
            "Saved",
            &[NoteLine {
                tone: NoteTone::Plain,
                text: "done".to_owned(),
            }],
        )?;
        let shown = screen(&session.terminal().written());
        assert!(shown.starts_with("▲  careful\n│\n◇  Saved "), "{shown}");
        assert!(shown.contains("│  done"), "{shown}");
        Ok(())
    }

    #[test]
    fn a_new_session_leaves_the_terminal_alone_and_writes_nothing() {
        let form = bare(Script::new(TerminalKind::Live, [], []));
        assert_eq!(form.terminal().switches(), []);
        assert_eq!(form.terminal().written(), "");
    }

    #[test]
    fn the_terminal_is_quiet_only_while_a_question_runs() -> Result<()> {
        let form = form(Script::new(TerminalKind::Live, [Key::Enter], []))?;
        assert_eq!(form.terminal().switches(), []);
        form.ask(confirm("Go?"))?;
        assert_eq!(
            form.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        let failed = form.ask(confirm("Again?"));
        assert!(failed.is_err(), "{failed:?}");
        assert_eq!(
            form.terminal().switches(),
            [
                Switch::Quieted,
                Switch::Restored,
                Switch::Quieted,
                Switch::Restored
            ]
        );
        Ok(())
    }

    fn typed_then_stopped() -> Script {
        Script::new(TerminalKind::Live, [Key::Char('a'), Key::Char('b')], [])
    }

    #[test]
    fn a_stop_before_the_question_ends_it_without_drawing() {
        let ui = bare(typed_then_stopped());
        let (abort, stop) = Abort::new();
        stop.stop();
        let asked = ui.ask(ask_text("Name").abort(&abort));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        assert_eq!(ui.terminal().written(), "");
        assert_eq!(ui.terminal().switches(), []);
    }

    #[test]
    fn a_stop_in_the_middle_leaves_the_cancelled_frame_and_the_terminal_normal() {
        let ui = bare(typed_then_stopped().stopped_at_key(2));
        let (abort, _stop) = Abort::new();
        let asked = ui.ask(ask_text("Name").abort(&abort));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        let raw = ui.terminal().written();
        let shown = screen(&raw);
        assert!(shown.ends_with("■  Name\n│  a\n│"), "{shown}");
        assert!(raw.ends_with(SHOW_CURSOR), "{raw:?}");
        assert_eq!(
            ui.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        let next = ui.ask(ask_text("Again"));
        assert!(matches!(next, Err(Error::Terminal(_))), "{next:?}");
    }

    #[test]
    fn a_stop_is_aborted_not_cancelled_or_interrupted() {
        let ui = bare(typed_then_stopped().stopped_at_key(1));
        let (abort, _stop) = Abort::new();
        let asked = ui.ask(ask_text("Name").abort(&abort)).cancel_as_none();
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        assert!(!Error::Aborted.is_cancel());
    }

    #[test]
    fn a_question_without_an_abort_ignores_a_simulated_stop() -> Result<()> {
        let ui =
            bare(Script::new(TerminalKind::Live, [Key::Enter], []).stopped_at_key(1));
        assert!(ui.ask(confirm("Go?"))?);
        Ok(())
    }

    #[test]
    fn the_abort_of_the_session_ends_every_question() {
        let (abort, stop) = Abort::new();
        let ui = bare(typed_then_stopped().stopped_at_key(2)).abort(abort);
        let asked = ui.ask(ask_text("Name"));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        stop.stop();
        let later = ui.ask(ask_text("Again"));
        assert!(matches!(later, Err(Error::Aborted)), "{later:?}");
    }

    #[test]
    fn a_tuple_stops_at_the_question_that_was_aborted() {
        let ui = bare(
            Script::new(
                TerminalKind::Live,
                [Key::Char('a'), Key::Enter, Key::Char('b'), Key::Enter],
                [],
            )
            .stopped_at_key(3),
        );
        let (abort, _stop) = Abort::new();
        let asked = ui.ask((ask_text("One"), ask_text("Two").abort(&abort)));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        let shown = screen(&ui.terminal().written());
        assert!(shown.contains("◇  One\n│  a"), "{shown}");
        assert!(shown.contains("■  Two"), "{shown}");
    }

    #[test]
    fn a_timeout_ends_a_question_nobody_answers() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Char('a')], []));
        let started = Instant::now();
        let asked = ui.ask(ask_text("Name").timeout(Duration::from_millis(40)));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        assert!(started.elapsed() >= Duration::from_millis(40));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("■  Name\n│  a\n│"), "{shown}");
    }

    #[test]
    fn a_timeout_that_does_not_run_out_changes_nothing() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let asked = ui.ask(confirm("Go?").timeout(Duration::from_secs(3600)))?;
        assert!(asked);
        Ok(())
    }

    #[test]
    fn a_timeout_that_ran_out_before_the_question_ends_it_at_once() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let asked = ui.ask(confirm("Go?").timeout(Duration::ZERO));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    fn dumb_or_piped(kind: TerminalKind) -> Script {
        Script::new(kind, [], ["first", "second"])
    }

    #[rstest]
    #[case::dumb(TerminalKind::Dumb)]
    #[case::piped(TerminalKind::Redirected)]
    fn a_stop_while_a_line_is_awaited_ends_the_question_and_takes_no_line(
        #[case] kind: TerminalKind,
    ) {
        let ui = bare(dumb_or_piped(kind).stopped_at_line(1));
        let (abort, _stop) = Abort::new();
        let asked = ui.ask(ask_text("Name").abort(&abort));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        let next = ui.ask(ask_text("Again"));
        assert!(matches!(&next, Ok(line) if line == "first"), "{next:?}");
    }

    #[rstest]
    #[case::dumb(TerminalKind::Dumb)]
    #[case::piped(TerminalKind::Redirected)]
    fn a_timeout_ends_a_line_nobody_types(#[case] kind: TerminalKind) {
        let ui = bare(Script::new(kind, [], []));
        let started = Instant::now();
        let asked = ui.ask(ask_text("Name").timeout(Duration::from_millis(40)));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    #[rstest]
    #[case::dumb(TerminalKind::Dumb)]
    #[case::piped(TerminalKind::Redirected)]
    fn a_stop_in_the_session_abort_ends_a_line_read(#[case] kind: TerminalKind) {
        let (abort, _stop) = Abort::new();
        let ui = bare(dumb_or_piped(kind).stopped_at_line(1)).abort(abort);
        let asked = ui.ask(ask_text("Name"));
        assert!(matches!(asked, Err(Error::Aborted)), "{asked:?}");
    }

    #[test]
    fn a_line_read_without_an_abort_ignores_a_simulated_stop() -> Result<()> {
        let ui = bare(dumb_or_piped(TerminalKind::Dumb).stopped_at_line(1));
        assert_eq!(ui.ask(ask_text("Name"))?, "first");
        Ok(())
    }

    #[test]
    fn a_given_answer_under_an_ended_abort_is_aborted_without_drawing() {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let (abort, stop) = Abort::new();
        stop.stop();
        let given = ui.ask(confirm("Go?").given(Some(true)).abort(&abort));
        assert!(matches!(given, Err(Error::Aborted)), "{given:?}");
        let line = ui.ask(confirm("Go?").given_line(Some("yes")).abort(&abort));
        assert!(matches!(line, Err(Error::Aborted)), "{line:?}");
        assert_eq!(ui.terminal().written(), "");
        assert_eq!(ui.ask(confirm("Go?").given(Some(true))).ok(), Some(true));
    }

    #[test]
    fn a_given_answer_under_an_ended_session_abort_is_aborted() {
        let (abort, stop) = Abort::new();
        let ui = bare(Script::new(TerminalKind::Live, [], [])).abort(abort);
        stop.stop();
        let given = ui.ask(confirm("Go?").given(Some(true)));
        assert!(matches!(given, Err(Error::Aborted)), "{given:?}");
        assert_eq!(ui.terminal().written(), "");
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn a_wait_is_quiet_only_while_it_runs() -> Result<()> {
        let form = form(Script::new(TerminalKind::Live, [], []))?;
        form.wait("Checking", async { 1 }).await?;
        assert_eq!(
            form.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        Ok(())
    }

    const ORANGE: Style = Style::new().fg(Color::Rgb(255, 120, 0));

    fn asked_go(settings: Settings, theme: Theme) -> Result<String> {
        let script = Script::new(TerminalKind::Live, [Key::Enter], []);
        let session = Session::new(script, settings).theme(theme);
        session.ask(confirm("Go?"))?;
        Ok(session.terminal().written())
    }

    #[test]
    fn a_custom_theme_changes_only_its_role() -> Result<()> {
        let theme = Theme::clack().active(ORANGE);
        let raw = asked_go(Settings::new(ColorChoice::Colored), theme)?;
        assert!(raw.contains("\x1b[38;2;255;120;0m◆\x1b[0m"), "{raw:?}");
        assert!(raw.contains("\x1b[38;2;255;120;0m│\x1b[0m"), "{raw:?}");
        assert!(!raw.contains("\x1b[36m"), "{raw:?}");
        assert!(raw.contains("\x1b[32m◇\x1b[0m"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn rgb_falls_back_to_the_palette_on_a_256_color_terminal() -> Result<()> {
        let theme = Theme::clack().active(ORANGE);
        let skin = Skin::new(ColorChoice::Colored).with_depth(ColorDepth::Ansi256);
        let raw = asked_go(
            Settings {
                skin,
                ..Settings::new(ColorChoice::Colored)
            },
            theme,
        )?;
        assert!(raw.contains("\x1b[38;5;208m◆\x1b[0m"), "{raw:?}");
        assert!(!raw.contains("38;2"), "{raw:?}");
        Ok(())
    }

    #[rstest]
    #[case::clack(Theme::clack())]
    #[case::mono(Theme::mono())]
    #[case::custom(Theme::clack().active(ORANGE).error(ORANGE.bold()))]
    fn plain_paints_nothing_with_any_theme(#[case] theme: Theme) -> Result<()> {
        let raw = asked_go(Settings::new(ColorChoice::Plain), theme)?;
        assert!(!raw.contains("\x1b[0m"), "{raw:?}");
        assert!(!raw.contains("m◆"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn ascii_symbols_draw_a_whole_form_without_non_ascii() -> Result<()> {
        let keys = [Key::Char('a'), Key::Enter, Key::Enter];
        let script = Script::new(TerminalKind::Live, keys, []);
        let session = bare(script).symbols(Symbols::ascii());
        session.intro("acme")?;
        session.ask((ask_text("Name"), confirm("Go?")))?;
        session.log(Tone::Warn, "careful")?;
        session.log(Tone::Success, "fine")?;
        session.note(
            "Saved",
            &[NoteLine {
                tone: NoteTone::Warn,
                text: "done".to_owned(),
            }],
        )?;
        session.outro("bye")?;
        let written = session.terminal().written();
        assert!(written.is_ascii(), "{written:?}");
        assert!(written.contains("T  "), "{written:?}");
        assert!(written.contains("o  Go?"), "{written:?}");
        Ok(())
    }

    fn german() -> Messages {
        Messages::english()
            .pick_an_option("Bitte mindestens eine Option wählen.")
            .keys_multiselect("↑/↓ Bewegen • Leertaste: wählen • Enter: weiter")
    }

    #[test]
    fn other_messages_replace_the_english_ones() -> Result<()> {
        let keys = [Key::Enter, Key::Char(' '), Key::Enter];
        let session =
            bare(Script::new(TerminalKind::Live, keys, [])).messages(german());
        let picked =
            session.ask(multiselect("Tools").item(1, "fmt").item(2, "lint"))?;
        let raw = session.terminal().written();
        assert_eq!(picked, [1]);
        assert!(
            raw.contains("Bitte mindestens eine Option wählen."),
            "{raw:?}"
        );
        assert!(!raw.contains("Please select"), "{raw:?}");
        assert!(raw.contains("Leertaste"), "{raw:?}");
        Ok(())
    }

    fn picked_after(keys: KeyMap, presses: [Key; 2]) -> Result<&'static str> {
        let session = bare(Script::new(TerminalKind::Live, presses, [])).keys(keys);
        session.ask(select("Pick").item("a", "A").item("b", "B").item("c", "C"))
    }

    #[test]
    fn j_moves_down_until_it_is_unbound() -> Result<()> {
        let presses = [Key::Char('j'), Key::Enter];
        assert_eq!(picked_after(KeyMap::clack(), presses)?, "b");
        let without = KeyMap::clack().unbind(Key::Char('j'));
        assert_eq!(picked_after(without, presses)?, "a");
        Ok(())
    }

    #[test]
    fn a_bound_key_takes_the_action_and_an_unbound_arrow_does_nothing() -> Result<()> {
        let keys = KeyMap::clack()
            .bind(Key::Char('n'), Action::Down)
            .unbind(Key::Down);
        assert_eq!(
            picked_after(keys.clone(), [Key::Char('n'), Key::Enter])?,
            "b"
        );
        assert_eq!(picked_after(keys, [Key::Down, Key::Enter])?, "a");
        Ok(())
    }

    #[test]
    fn text_keeps_its_letters_whatever_the_map() -> Result<()> {
        let keys = [Key::Char('j'), Key::Char(' '), Key::Char('a'), Key::Enter];
        let session = bare(Script::new(TerminalKind::Live, keys, []))
            .keys(KeyMap::clack().unbind(Key::Char('j')));
        assert_eq!(session.ask(ask_text("Name"))?, "j a");
        Ok(())
    }

    #[test]
    fn without_rail_draws_no_rail() -> Result<()> {
        let keys = [Key::Char('a'), Key::Enter, Key::Enter, Key::Enter];
        let session = bare(Script::new(TerminalKind::Live, keys, [])).without_rail();
        session.intro("acme")?;
        session.ask((
            ask_text("Name"),
            confirm("Go?"),
            select("Pick").item(1, "one").item(2, "two"),
        ))?;
        session.log(Tone::Warn, "careful")?;
        session.outro("bye")?;
        let written = session.terminal().written();
        for glyph in ['│', '┌', '└'] {
            assert!(!written.contains(glyph), "{glyph}: {written:?}");
        }
        assert!(written.contains("bye"), "{written:?}");
        Ok(())
    }

    #[test]
    fn symbols_set_after_without_rail_keep_it_railless() -> Result<()> {
        let session = bare(Script::new(TerminalKind::Live, [], []))
            .without_rail()
            .symbols(Symbols::ascii());
        session.outro("bye")?;
        assert_eq!(screen(&session.terminal().written()), "bye");
        Ok(())
    }

    #[test]
    fn accessible_output_has_no_escapes_and_asks_once() -> Result<()> {
        let keys = [Key::Down, Key::Down, Key::Up, Key::Enter];
        let session = bare(Script::new(TerminalKind::Live, keys, []))
            .output(OutputMode::Accessible);
        session.intro("acme")?;
        let picked = session.ask(
            select("Pick")
                .item(1, "one")
                .item(2, "two")
                .item(3, "three"),
        )?;
        session.outro("bye")?;
        let written = session.terminal().written();
        assert_eq!(picked, 2);
        assert!(!written.contains('\x1b'), "{written:?}");
        assert_eq!(written.matches("◆  Pick").count(), 1, "{written}");
        assert!(written.contains("◇  Pick\n│  two\n"), "{written}");
        Ok(())
    }

    #[test]
    fn accessible_output_prints_a_refusal_as_a_new_line() -> Result<()> {
        let keys = [Key::Enter, Key::Char(' '), Key::Enter];
        let session = bare(Script::new(TerminalKind::Live, keys, []))
            .output(OutputMode::Accessible);
        session.ask(multiselect("Tools").item(1, "fmt").item(2, "lint"))?;
        let written = session.terminal().written();
        assert!(!written.contains('\x1b'), "{written:?}");
        assert!(
            written.contains("Please select at least one option."),
            "{written}"
        );
        Ok(())
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn accessible_wait_prints_its_line_once() -> Result<()> {
        let session = bare(Script::new(TerminalKind::Live, [], []))
            .output(OutputMode::Accessible);
        session.wait("Checking", std::future::ready(7)).await?;
        let written = session.terminal().written();
        assert_eq!(written, "│\n◒  Checking...\n");
        Ok(())
    }

    /// What painting `rows` writes over the picture the painter holds.
    fn painted(painter: &mut Painter, width: Option<usize>, rows: &[&str]) -> String {
        painter.frame.reset(width);
        painter
            .frame
            .extend(rows.iter().map(|row| (*row).to_owned()));
        painter.paint(width).to_owned()
    }

    #[rstest]
    #[case::the_first_picture(&[], &["a", "b"], "\r\x1b[J" , "a\nb\n")]
    #[case::nothing_changed(&["a", "b"], &["a", "b"], "", "")]
    #[case::one_row_changed(&["a", "b", "c"], &["a", "x", "c"], "\x1b[2A", "\r\x1b[2Kx\x1b[2B\r")]
    #[case::the_last_row_grows_a_row(&["a", "b"], &["a", "b", "c"], "", "\r\x1b[2Kc\n\r")]
    #[case::rows_vanish(&["a", "b", "c"], &["a"], "", "\x1b[2A\r\x1b[J")]
    #[case::a_row_changes_and_one_is_added(&["a", "b"], &["x", "b", "c"], "\x1b[2A", "\r\x1b[2Kx\x1b[2B\r\x1b[2Kc\n\r")]
    fn only_the_rows_that_differ_are_written(
        #[case] shown: &[&str],
        #[case] next: &[&str],
        #[case] head: &str,
        #[case] tail: &str,
    ) {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        painted(&mut painter, Some(80), shown);
        let written = painted(&mut painter, Some(80), next);
        assert_eq!(written, format!("{head}{tail}"));
    }

    #[test]
    fn a_new_width_paints_everything_again() {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        painted(&mut painter, Some(80), &["a", "b"]);
        let written = painted(&mut painter, Some(40), &["a", "b"]);
        assert_eq!(written, "\r\x1b[2A\x1b[Ja\nb\n");
    }

    #[test]
    fn a_narrower_width_goes_up_by_the_reflowed_rows() {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        painted(&mut painter, Some(10), &["abcdefgh", "ij"]);
        // The first row of 8 columns wraps in 4: 2 + 1 rows on the screen.
        let written = painted(&mut painter, Some(4), &["abcd", "ij"]);
        assert_eq!(written, "\r\x1b[3A\x1b[Jabcd\nij\n");
    }

    #[test]
    fn rows_are_cut_to_the_width_so_the_repaint_count_stays_right() {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        let written = painted(&mut painter, Some(6), &["abcdefghij", "kl"]);
        assert_eq!(written, "\r\x1b[Jabcde…\nkl\n");
        assert_eq!(painter.shown, ["abcde…", "kl"]);
        let mut piped =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Accessible);
        assert_eq!(
            painted(&mut piped, Some(6), &["abcdefghij"]),
            "abcdefghij\n"
        );
    }

    #[test]
    fn leaving_wipes_the_picture_and_leaves_the_text() {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        painted(&mut painter, Some(80), &["a", "b"]);
        assert_eq!(painter.leave(Some(80), "done\n"), "\r\x1b[2A\x1b[Jdone\n");
        assert_eq!(painter.shown, Vec::<String>::new());
    }

    #[test]
    fn leaving_after_a_resize_goes_up_by_the_rows_at_the_new_width() {
        let mut painter =
            Painter::new(Skin::new(ColorChoice::Plain), OutputMode::Animated);
        painted(&mut painter, Some(10), &["abcdefgh", "ij"]);
        // At 4 columns the first row takes 2 screen rows, the second 1.
        assert_eq!(painter.leave(Some(4), "done\n"), "\r\x1b[3A\x1b[Jdone\n");
    }

    #[test]
    fn answering_after_a_resize_without_a_repaint_wipes_the_reflowed_rows() {
        let script =
            Script::new(TerminalKind::Live, [Key::Enter], []).resized_at_key(1, 20);
        let ui = bare(script);
        let asked = ui.ask(ask_text("Name").initial("x".repeat(30)));
        assert_eq!(asked.ok().as_deref(), Some(&*"x".repeat(30)));
        let raw = ui.terminal().written();
        assert!(raw.contains("\r\x1b[4A\x1b[J"), "{raw:?}");
    }

    #[test]
    fn a_long_answer_is_wrapped_before_it_is_clipped() {
        let long = "x".repeat(100);
        let script = Script::new(TerminalKind::Live, [Key::Enter], []);
        let ui = bare(script);
        assert!(ui.ask(ask_text("Name").initial(long)).is_ok());
        let frames = ui.terminal().frames();
        let first = frames.iter().find(|frame| frame.contains('x'));
        let first = first.map(String::as_str).unwrap_or_default();
        assert!(!first.contains('…'), "{first}");
        assert_eq!(first.matches('x').count(), 100, "{first}");
    }

    fn defaulting(kind: TerminalKind) -> Session<Script> {
        bare(Script::new(kind, [], [])).defaults(Defaults::Take)
    }

    #[test]
    fn defaults_answer_by_the_default_and_show_the_answered_frame() -> Result<()> {
        let ui = defaulting(TerminalKind::Redirected);
        let (name, go, kind, tools) = ui.ask((
            ask_text("Name").default("Ada"),
            confirm("Go?").default(Reply::No),
            select("Kind").item(1_u8, "A").item(2, "B").default_at(1),
            multiselect("Tools")
                .item("fmt", "Formatter")
                .item("lint", "Linter")
                .picked([1]),
        ))?;
        assert_eq!(
            (name.as_str(), go, kind, tools),
            ("Ada", false, 2, vec!["lint"])
        );
        let shown = screen(&ui.terminal().written());
        for frame in [
            "◇  Name\n│  Ada",
            "◇  Go?\n│  No",
            "◇  Kind\n│  B",
            "◇  Tools\n│  Linter",
        ] {
            assert!(shown.contains(frame), "{frame}: {shown}");
        }
        assert!(!shown.contains('◆'), "{shown}");
        Ok(())
    }

    #[test]
    fn defaults_without_a_default_name_the_question() {
        let ui = defaulting(TerminalKind::Live);
        let cases = [
            ui.ask(ask_text("Email")).map(drop),
            ui.ask(password("Secret")).map(drop),
            ui.ask(select("Kind").item(1, "A").item(2, "B")).map(drop),
            ui.ask(multiselect("Tools").item(1, "A")).map(drop),
        ];
        let names: Vec<String> = cases
            .into_iter()
            .map(|asked| match asked {
                Err(Error::NoDefault { question }) => question,
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(names, ["Email", "Secret", "Kind", "Tools"]);
    }

    #[test]
    fn defaults_still_pass_through_the_checks() {
        let ui = defaulting(TerminalKind::Redirected);
        let asked = ui.ask(ask_text("Name").default("root").validate(|name: &str| {
            if name == "root" {
                Err("reserved")
            } else {
                Ok(())
            }
        }));
        assert!(
            matches!(&asked, Err(Error::Refused { question, reason })
                if question == "Name" && reason == "reserved"),
            "{asked:?}"
        );
    }

    #[test]
    fn defaults_take_a_lone_option_without_a_default() -> Result<()> {
        let ui = defaulting(TerminalKind::Redirected);
        assert_eq!(ui.ask(select("Kind").item(7_u8, "Only"))?, 7);
        Ok(())
    }

    #[test]
    fn a_question_while_a_spinner_runs_is_busy() -> Result<()> {
        let ui = bare(Script::new(
            TerminalKind::Live,
            [Key::Enter, Key::Enter],
            [],
        ));
        let mut spinner = ui.spinner();
        spinner.start("Working")?;
        let asked = ui.ask(confirm("Go?"));
        assert!(matches!(asked, Err(Error::Busy)), "{asked:?}");
        let given = ui.ask(confirm("Go?").given(Some(true)));
        assert!(matches!(given, Err(Error::Busy)), "{given:?}");
        spinner.stop("Done")?;
        assert!(ui.ask(confirm("Go?"))?);
        let mut dropped = ui.spinner();
        dropped.start("Again")?;
        drop(dropped);
        assert!(ui.ask(confirm("Go?"))?);
        Ok(())
    }

    #[test]
    fn the_cursor_comes_back_when_the_question_fails() {
        let cancelled = bare(Script::new(TerminalKind::Live, [Key::Cancel], []));
        assert!(cancelled.ask(confirm("Go?")).is_err());
        let cut_off = bare(Script::new(TerminalKind::Live, [], []));
        let asked = cut_off.ask(confirm("Go?"));
        assert!(matches!(asked, Err(Error::Terminal(_))), "{asked:?}");
        for ui in [cancelled, cut_off] {
            let raw = ui.terminal().written();
            assert!(raw.contains(HIDE_CURSOR), "{raw:?}");
            assert!(raw.ends_with(SHOW_CURSOR), "{raw:?}");
        }
    }

    #[test]
    #[expect(clippy::panic, reason = "a check that panics is the case under test")]
    fn the_cursor_comes_back_when_a_check_panics() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let prompt = select("Kind")
            .item(1, "A")
            .validate(|_| -> Result<(), &str> { panic!("the check broke") });
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ui.ask(prompt).map(drop)
        }));
        assert!(outcome.is_err());
        let raw = ui.terminal().written();
        assert!(raw.ends_with(SHOW_CURSOR), "{raw:?}");
    }

    #[rstest]
    #[case::dumb(TerminalKind::Dumb)]
    #[case::piped(TerminalKind::Redirected)]
    fn an_empty_line_keeps_the_initial_and_the_default_of_a_text(
        #[case] kind: TerminalKind,
    ) -> Result<()> {
        let ui = |lines: &'static [&'static str]| {
            bare(Script::new(kind, [], lines.iter().copied()))
        };
        assert_eq!(ui(&[""]).ask(ask_text("N").initial("ab"))?, "ab");
        assert_eq!(ui(&["x"]).ask(ask_text("N").initial("ab"))?, "x");
        assert_eq!(ui(&[""]).ask(ask_text("N").default("fb"))?, "fb");
        assert_eq!(
            ui(&[" "]).ask(ask_text("N").initial("ab").default("fb"))?,
            "fb"
        );
        Ok(())
    }

    #[test]
    fn an_empty_line_takes_the_start_of_the_other_prompts_on_a_dumb_terminal()
    -> Result<()> {
        let ui = || bare(Script::new(TerminalKind::Dumb, [], [""]));
        let kinds = || select("K").item('a', "A").item('b', "B").default_at(1);
        assert_eq!(ui().ask(kinds())?, 'b');
        let many = multiselect("M").item('a', "A").item('b', "B").picked([1]);
        assert_eq!(ui().ask(many)?, ['b']);
        assert!(!ui().ask(confirm("C").default(Reply::No))?);
        Ok(())
    }

    #[test]
    fn a_piped_stdin_never_waits_for_keys() {
        let ui = bare(Script::new(TerminalKind::Redirected, [Key::Enter], []));
        let asked = ui.ask(select("Kind").item(1, "A").item(2, "B"));
        assert!(
            matches!(asked, Err(Error::NoInput { ref question }) if question == "Kind"),
            "{asked:?}"
        );
        let ended = bare(Script::new(TerminalKind::Redirected, [], []));
        let ended_asked = ended.ask(ask_text("Name"));
        assert!(
            matches!(ended_asked, Err(Error::NoInput { ref question }) if question == "Name"),
            "{ended_asked:?}"
        );
        let piped = bare(Script::new(TerminalKind::Redirected, [], ["Ada"]));
        assert_eq!(piped.ask(ask_text("Name")).ok().as_deref(), Some("Ada"));
        assert!(!piped.terminal().written().contains(HIDE_CURSOR));
    }

    #[test]
    fn a_plain_line_answers_a_select_by_number_or_label() -> Result<()> {
        let ui = bare(Script::new(
            TerminalKind::Dumb,
            [],
            ["2", "  library ", "1"],
        ));
        let pick = || select("Kind").item("lib", "Library").item("bin", "Binary");
        assert_eq!(ui.ask(pick())?, "bin");
        assert_eq!(ui.ask(pick())?, "lib");
        let by_default = || pick().default_at(1);
        assert_eq!(ui.ask(by_default())?, "lib");
        Ok(())
    }
}
