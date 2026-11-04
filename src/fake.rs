//! A scripted terminal and a small screen emulator, for testing programs that
//! use clacky.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    rc::Rc,
    thread,
};

use crate::{
    terminal::{
        LineRead,
        Press,
        Terminal,
        TerminalKind,
        abort::Abort,
        signals::Catching,
    },
    values::line::Key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A change of the terminal's mode that the script saw.
pub enum Switch {
    /// The terminal was quieted: raw keys, no echo.
    Quieted,
    /// The quiet guard was dropped and the terminal was put back.
    Restored,
}

#[derive(Debug)]
/// The guard of a quieted [`Script`]; dropping it records a restore.
pub struct Quieted(Rc<RefCell<Vec<Switch>>>);

// GUARD: records that the terminal is back to normal.
impl Drop for Quieted {
    fn drop(&mut self) {
        self.0.borrow_mut().push(Switch::Restored);
    }
}

#[derive(Debug)]
/// A scripted terminal. It plays back the keys and lines it is given and
/// records everything written.
pub struct Script {
    kind: TerminalKind,
    width: Cell<Option<usize>>,
    reflow: Option<Reflow>,
    stop_key: Option<usize>,
    keys_read: Cell<usize>,
    stop_line: Option<usize>,
    lines_read: Cell<usize>,
    keystrokes: RefCell<VecDeque<Key>>,
    lines: RefCell<VecDeque<String>>,
    written: RefCell<String>,
    /// Where each `write` call ended in `written`, so `frames` can split it.
    ends: RefCell<Vec<usize>>,
    hidden: Cell<usize>,
    discarded: Cell<usize>,
    echo_works: bool,
    ctrl_c: CtrlC,
    switches: Rc<RefCell<Vec<Switch>>>,
}

#[derive(Debug, Clone, Copy)]
struct Reflow {
    key: usize,
    width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtrlC {
    Never,
    AtOnce,
    Before,
}

impl Script {
    /// A terminal of `kind` that plays back `strokes` as keys and `lines` as
    /// stdin lines. Its width is 80 columns. With no input left, a read fails
    /// with [`io::ErrorKind::UnexpectedEof`] (keys) or ends the input (lines).
    #[must_use]
    pub fn new(
        kind: TerminalKind,
        strokes: impl IntoIterator<Item = Key>,
        lines: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            kind,
            width: Cell::new(Some(80)),
            reflow: None,
            stop_key: None,
            keys_read: Cell::new(0),
            stop_line: None,
            lines_read: Cell::new(0),
            keystrokes: RefCell::new(strokes.into_iter().collect()),
            lines: RefCell::new(lines.into_iter().map(str::to_owned).collect()),
            written: RefCell::new(String::new()),
            ends: RefCell::new(Vec::new()),
            hidden: Cell::new(0),
            discarded: Cell::new(0),
            echo_works: true,
            ctrl_c: CtrlC::Never,
            switches: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Makes the interrupt counter read one from the start, as if Ctrl-C came
    /// before the session.
    #[must_use]
    pub fn interrupted_before(self) -> Self {
        Self {
            ctrl_c: CtrlC::Before,
            ..self
        }
    }

    /// Resizes the terminal to `width` columns when the `key`th key (counting
    /// from 1) is read.
    #[must_use]
    pub fn resized_at_key(self, key: usize, width: usize) -> Self {
        Self {
            reflow: Some(Reflow { key, width }),
            ..self
        }
    }

    /// Calls the stop of the question when the `key`th key (counting from 1)
    /// is awaited, as if another thread had done it while the person was idle.
    /// The question ends with [`Error::Aborted`](crate::Error::Aborted) and
    /// that key is not taken. A question without an abort is not affected.
    #[must_use]
    pub fn stopped_at_key(self, key: usize) -> Self {
        Self {
            stop_key: Some(key),
            ..self
        }
    }

    /// Calls the stop of the question when the `line`th line (counting from 1)
    /// is awaited, as if another thread had done it while nothing was typed.
    /// The question ends with [`Error::Aborted`](crate::Error::Aborted) and
    /// that line is not taken. A question without an abort is not affected.
    #[must_use]
    pub fn stopped_at_line(self, line: usize) -> Self {
        Self {
            stop_line: Some(line),
            ..self
        }
    }

    /// Resizes the terminal to `width` columns at once.
    pub fn resize(&self, width: usize) {
        self.width.set(Some(width));
    }

    /// The mode switches so far, in order.
    #[must_use]
    pub fn switches(&self) -> Vec<Switch> {
        self.switches.borrow().clone()
    }

    /// Makes Ctrl-C arrive at the first read.
    #[must_use]
    pub fn pressing_ctrl_c(self) -> Self {
        Self {
            ctrl_c: CtrlC::AtOnce,
            ..self
        }
    }

    /// How many times unread input was discarded.
    #[must_use]
    pub fn discards(&self) -> usize {
        self.discarded.get()
    }

    /// Makes the terminal unable to hide the echo, as some terminals are.
    #[must_use]
    pub fn without_hidden_echo(self) -> Self {
        Self {
            echo_works: false,
            ..self
        }
    }

    /// Everything written so far, escapes included.
    #[must_use]
    pub fn written(&self) -> String {
        self.written.borrow().clone()
    }

    /// The screen after each `write` call, one frame per picture.
    #[must_use]
    pub fn frames(&self) -> Vec<String> {
        let written = self.written.borrow();
        let mut screen = Screen::default();
        let mut from = 0;
        self.ends
            .borrow()
            .iter()
            .map(|end| {
                let part = written.get(from..*end).unwrap_or_default();
                screen = part.chars().fold(std::mem::take(&mut screen), Screen::feed);
                from = *end;
                screen.text()
            })
            .collect()
    }

    /// How many hidden reads, such as password input, were made.
    #[must_use]
    pub fn hidden_reads(&self) -> usize {
        self.hidden.get()
    }
}

impl Terminal for Script {
    type QuietTerminal = Quieted;

    fn quiet(&self) -> Quieted {
        self.switches.borrow_mut().push(Switch::Quieted);
        Quieted(Rc::clone(&self.switches))
    }

    fn interrupts(&self) -> u64 {
        u64::from(self.ctrl_c == CtrlC::Before)
    }

    fn catch_interrupts(&self) -> Catching {
        Catching::nothing()
    }

    fn kind(&self) -> TerminalKind {
        self.kind
    }

    fn has_color(&self) -> bool {
        false
    }

    fn width(&self) -> Option<usize> {
        self.width.get()
    }

    fn write(&self, text: &str) -> io::Result<()> {
        let mut written = self.written.borrow_mut();
        written.push_str(text);
        self.ends.borrow_mut().push(written.len());
        Ok(())
    }

    fn keystroke(&self, abort: &Abort) -> io::Result<Press> {
        self.keys_read.set(self.keys_read.get() + 1);
        if self.stop_key == Some(self.keys_read.get()) {
            abort.fire();
        }
        if abort.ended() {
            return Ok(Press::Aborted);
        }
        if let Some(reflow) = self
            .reflow
            .filter(|reflow| reflow.key == self.keys_read.get())
        {
            self.width.set(Some(reflow.width));
        }
        let next = self.keystrokes.borrow_mut().pop_front();
        match (next, abort.remaining()) {
            (Some(key), _) => Ok(Press::Key(key)),
            // Nobody types: a real terminal waits for the deadline.
            (None, Some(left)) => {
                thread::sleep(left);
                Ok(Press::Aborted)
            }
            (None, None) => Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
        }
    }

    fn line(&self, abort: &Abort) -> io::Result<LineRead> {
        self.lines_read.set(self.lines_read.get() + 1);
        if self.stop_line == Some(self.lines_read.get()) {
            abort.fire();
        }
        if abort.ended() {
            return Ok(LineRead::Aborted);
        }
        let next = self.lines.borrow_mut().pop_front();
        match (next, abort.remaining()) {
            (Some(line), _) => Ok(LineRead::Line(line)),
            // Nobody types: a real terminal waits for the deadline.
            (None, Some(left)) => {
                thread::sleep(left);
                Ok(LineRead::Aborted)
            }
            (None, None) => Ok(LineRead::End),
        }
    }

    fn hidden<R>(&self, work: impl FnOnce(&Self) -> R) -> io::Result<R> {
        if self.echo_works {
            self.hidden.set(self.hidden.get() + 1);
            Ok(work(self))
        } else {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }

    fn discard_typeahead(&self) {
        self.discarded.set(self.discarded.get() + 1);
    }

    #[cfg(feature = "tokio")]
    fn interrupt(&self) -> impl Future<Output = ()> {
        let never = (self.ctrl_c == CtrlC::Never).then(std::future::pending::<()>);
        async move {
            if let Some(pending) = never {
                pending.await;
            }
        }
    }
}

/// What the screen shows after replaying the escapes in `raw`.
#[must_use]
pub fn screen(raw: &str) -> String {
    raw.chars().fold(Screen::default(), Screen::feed).text()
}

/// `raw` with its terminal escapes removed.
#[must_use]
pub fn without_escapes(raw: &str) -> String {
    raw.chars()
        .scan(false, |in_escape, c| {
            let (next, keep) = match (*in_escape, c) {
                (false, '\x1b') => (true, None),
                (false, c) => (false, Some(c)),
                (true, 'A'..='Z' | 'a'..='z') => (false, None),
                (true, _) => (true, None),
            };
            *in_escape = next;
            Some(keep)
        })
        .flatten()
        .collect()
}

#[derive(Debug, Default)]
struct Screen {
    lines: Vec<Vec<char>>,
    line_index: usize,
    column: usize,
    escape: Option<String>,
}

impl Screen {
    fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().collect::<String>().trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_owned()
    }

    fn feed(self, c: char) -> Self {
        match (self.escape.as_ref(), c) {
            (Some(_), c) => self.in_escape(c),
            (None, '\x1b') => Self {
                escape: Some(String::new()),
                ..self
            },
            (None, '\n') => {
                let next = self.line_index + 1;
                self.at(next, 0)
            }
            (None, '\r') => {
                let current = self.line_index;
                self.at(current, 0)
            }
            (None, c) => self.put(c),
        }
    }

    fn in_escape(self, c: char) -> Self {
        let seq = self.escape.clone().unwrap_or_default() + c.encode_utf8(&mut [0; 4]);
        match c {
            'A'..='Z' | 'a'..='z' => Self {
                escape: None,
                ..self
            }
            .follow(&seq),
            _ => Self {
                escape: Some(seq),
                ..self
            },
        }
    }

    fn follow(self, seq: &str) -> Self {
        let count = |s: &str| s.parse::<usize>().unwrap_or(1);
        let (line_index, col) = (self.line_index, self.column);
        match seq.strip_prefix('[') {
            Some("2K") => self.clear_line(),
            Some("J") => self.clear_below(),
            Some(rest) if rest.ends_with('A') => {
                let up = count(rest.trim_end_matches('A'));
                self.at(line_index.saturating_sub(up), col)
            }
            Some(rest) if rest.ends_with('B') => {
                let down = count(rest.trim_end_matches('B'));
                self.at(line_index + down, col)
            }
            Some(rest) if rest.ends_with('C') => {
                let right = count(rest.trim_end_matches('C'));
                self.at(line_index, col + right)
            }
            _ => self,
        }
    }

    fn at(self, line_index: usize, col: usize) -> Self {
        let lines = grown(self.lines, line_index + 1);
        Self {
            lines,
            line_index,
            column: col,
            ..self
        }
    }

    fn clear_below(self) -> Self {
        let (at, col) = (self.line_index, self.column);
        let mut lines = edited(self.lines, at, |line| line.truncate(col));
        lines.truncate(at + 1);
        Self { lines, ..self }
    }
    fn clear_line(self) -> Self {
        let lines = edited(self.lines, self.line_index, Vec::clear);
        Self { lines, ..self }
    }

    fn put(self, c: char) -> Self {
        let col = self.column;
        let lines = edited(self.lines, self.line_index, |line| {
            line.resize(line.len().max(col + 1), ' ');
            line.get_mut(col).into_iter().for_each(|cell| *cell = c);
        });
        Self {
            lines,
            column: col + 1,
            ..self
        }
    }
}

fn grown(mut lines: Vec<Vec<char>>, len: usize) -> Vec<Vec<char>> {
    lines.resize(lines.len().max(len), Vec::new());
    lines
}

fn edited(
    mut lines: Vec<Vec<char>>,
    index: usize,
    edit: impl FnMut(&mut Vec<char>),
) -> Vec<Vec<char>> {
    lines = grown(lines, index + 1);
    lines.get_mut(index).into_iter().for_each(edit);
    lines
}

#[cfg(test)]
pub(crate) mod testing {
    use super::Script;
    use crate::{
        core::{
            ask::{Frame, Next, Phase, Prompt},
            error::{Error, Result},
            session::Session,
            settings::Settings,
        },
        render::color::ColorChoice,
        values::line::Key,
    };

    pub(crate) fn bare(script: Script) -> Session<Script> {
        Session::new(script, Settings::new(ColorChoice::Plain))
    }

    /// Turns a string into keys. `\n` Enter, `↑` `↓` `←` `→` arrows, `⇥` Tab,
    /// `⌫` Backspace, `⇤` Home, `⇲` End, `⇞` Ctrl-U, `⇟` Ctrl-K, `⌁` Ctrl-D,
    /// `␛` Esc, `☠` Ctrl-C; anything else types itself.
    pub(crate) fn keys(spec: &str) -> Vec<Key> {
        spec.chars()
            .map(|c| match c {
                '\n' => Key::Enter,
                '↑' => Key::Up,
                '↓' => Key::Down,
                '←' => Key::Left,
                '→' => Key::Right,
                '⇥' => Key::Tab,
                '⌫' => Key::Backspace,
                '⇤' => Key::Home,
                '⇲' => Key::End,
                '⇞' => Key::ClearBefore,
                '⇟' => Key::ClearAfter,
                '⌁' => Key::EndOfInput,
                '␛' => Key::Cancel,
                '☠' => Key::Interrupt,
                other => Key::Char(other),
            })
            .collect()
    }

    pub(crate) fn after<P: Prompt>(
        prompt: &P,
        keys: impl IntoIterator<Item = Key>,
    ) -> Option<P::State> {
        keys.into_iter()
            .try_fold(prompt.start().ok()?, |state, key| {
                match prompt.update(state, key) {
                    Next::Editing(state)
                    | Next::Refuse(state, _)
                    | Next::Submit(state)
                    | Next::Quit(state, _) => Some(state),
                }
            })
    }

    pub(crate) fn rendered<P: Prompt>(
        prompt: &P,
        keys: impl IntoIterator<Item = Key>,
        phase: Phase<'_>,
        color: ColorChoice,
        width: usize,
    ) -> Vec<String> {
        let mut frame = Frame::new(color, Some(width));
        if let Some(state) = after(prompt, keys) {
            prompt.view(&state, phase, &mut frame);
        }
        frame.rows().to_vec()
    }

    pub(crate) fn outcome<T>(
        result: &Result<T>,
        show: impl FnOnce(&T) -> String,
    ) -> String {
        match result {
            Ok(value) => show(value),
            Err(Error::Interrupted) => "interrupted".to_owned(),
            Err(Error::Cancelled) => "cancelled".to_owned(),
            Err(Error::NoInput) => "no input".to_owned(),
            Err(other) => format!("other: {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::fake::screen;

    #[test]
    fn screen_follows_cursor_moves_and_clears() {
        assert_eq!(screen("a\nb\x1b[1A\r\x1b[2Kc"), "c\nb");
        assert_eq!(screen("\x1b[2mdim\x1b[0m"), "dim");
        assert_eq!(screen("x\n\ny\x1b[1B"), "x\n\ny");
    }
}

/// Timing harness for `just bench`; ignored in normal test runs.
#[cfg(test)]
mod bench {
    use std::time::{Duration, Instant};

    use crate::{
        core::{
            ask::{Key, Prompt},
            session::Session,
            settings::Settings,
        },
        fake::Script,
        prompts::{
            autocomplete::autocomplete,
            choice::Choice,
            multiselect::multiselect,
            select::select,
            text::text,
        },
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    const KEYS: usize = 10_000;

    fn labels(count: usize) -> impl Iterator<Item = Choice<usize>> {
        (0..count).map(|at| Choice::new(at, format!("item number {at}")))
    }

    fn keys(cycle: &[Key]) -> Vec<Key> {
        cycle.iter().copied().cycle().take(KEYS).collect()
    }

    #[expect(clippy::print_stderr, reason = "the harness prints its numbers")]
    fn run<P: Prompt>(name: &str, make: impl Fn() -> P, keys: &[Key]) {
        let bare = timed(|| drop(make().simulate(keys.iter().copied())));
        let session = Session::new(
            Script::new(TerminalKind::Live, keys.iter().copied(), []),
            Settings::new(ColorChoice::Plain),
        );
        let drawn = timed(|| drop(session.ask(make())));
        let per_second = |took: Duration| {
            u32::try_from(keys.len())
                .map_or(0.0, |count| f64::from(count) / took.as_secs_f64())
        };
        eprintln!(
            "{name:<28} update {:>12.0} keys/s   redraw {:>12.0} keys/s   {} bytes",
            per_second(bare),
            per_second(drawn),
            session.terminal().written().len(),
        );
    }

    fn timed(work: impl FnOnce()) -> Duration {
        let start = Instant::now();
        work();
        start.elapsed()
    }

    #[test]
    #[ignore = "timing only: run with `just bench`"]
    fn keys_per_second() {
        let down = [Key::Down];
        run(
            "select (1000 items)",
            || select("Pick").items(labels(1000)),
            &keys(&down),
        );
        run(
            "multiselect (1000 items)",
            || multiselect("Pick").items(labels(1000)),
            &keys(&[Key::Char(' '), Key::Down]),
        );
        let typing = [
            Key::Char('i'),
            Key::Char('t'),
            Key::Char('e'),
            Key::Char('m'),
            Key::Char('9'),
            Key::Backspace,
            Key::Backspace,
            Key::Down,
            Key::Backspace,
            Key::Backspace,
            Key::Backspace,
        ];
        run(
            "autocomplete (10000 items)",
            || autocomplete("Find").items(labels(10_000)),
            &keys(&typing),
        );
        run(
            "text",
            || text("Name"),
            &keys(&[
                Key::Char('a'),
                Key::Char('b'),
                Key::Backspace,
                Key::Backspace,
            ]),
        );
    }
}
