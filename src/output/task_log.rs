//! `task_log`: the last lines of a noisy task, cleared when it succeeds.

use std::num::NonZeroUsize;

use unicode_width::UnicodeWidthChar as _;

use crate::{
    core::{error::Result, session::Session, settings::OutputMode},
    render::{
        color::escape_len,
        draw::{self, HIDE_CURSOR, SHOW_CURSOR},
        style::Skin,
    },
    terminal::{Terminal, TerminalKind},
};

const GUTTER: usize = 3;

const TAB: usize = 8;

const REPAINTED_LINES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drawing {
    /// Redrawn in place on a live terminal.
    Repaint,
    /// Accessible: each new row is written once, as a new line.
    Changes,
    /// Only the end is written.
    End,
}

#[derive(Debug)]
enum Header {
    /// The lines before the first group.
    Bare,
    Running(String),
    Succeeded(String),
    Failed(String),
}

#[derive(Debug, Clone, Copy)]
enum Change {
    /// The lines of a section from the one at `.1` on.
    Lines(usize, usize),
    /// The header of a section.
    Header(usize),
}

#[derive(Debug)]
struct Section {
    header: Header,
    lines: Vec<String>,
}

impl Section {
    fn header_row(&self, skin: &Skin) -> Option<String> {
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let text = match &self.header {
            Header::Bare => return None,
            Header::Running(name) => name.clone(),
            Header::Succeeded(message) => {
                format!("{} {message}", skin.paint(theme.success, symbols.success))
            }
            Header::Failed(message) => {
                format!("{} {message}", skin.paint(theme.error, symbols.error))
            }
        };
        Some(format!("{}  {text}", bar(skin)))
    }

    fn rows(
        &self,
        skin: &Skin,
        room: Option<usize>,
        to_hide: &mut usize,
    ) -> Vec<String> {
        let hidden = self.lines.len().min(*to_hide);
        *to_hide = to_hide.saturating_sub(hidden);
        self.header_row(skin)
            .into_iter()
            .chain(
                self.lines
                    .iter()
                    .skip(hidden)
                    .map(|line| line_row(skin, room, line)),
            )
            .collect()
    }
}

fn bar(skin: &Skin) -> String {
    skin.paint(skin.theme().rail, skin.symbols().bar)
}

fn line_row(skin: &Skin, room: Option<usize>, line: &str) -> String {
    let clipped = room.map_or_else(
        || line.to_owned(),
        |room| draw::clip_row(line, room, skin.symbols().ellipsis),
    );
    format!("{}  {}", bar(skin), skin.paint(skin.theme().hint, &clipped))
}

/// A title over a log that shows its last lines while the task runs. Nothing is
/// drawn before the first line, or on a terminal that is not live until the
/// end; success clears the log and leaves one line, failure leaves the whole
/// log.
///
/// ```no_run
/// # use clacky::{core::session::Session, terminal::Terminal};
/// # fn build<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// let mut log = session.task_log("Building");
/// log.message("compiling a")?;
/// log.group("tests").message("test one")?;
/// log.success("Built")?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct TaskLog<'s, T: Terminal> {
    session: &'s Session<T>,
    title: String,
    limit: Option<NonZeroUsize>,
    sections: Vec<Section>,
    drawn: Option<Vec<String>>,
    quiet: Option<T::QuietTerminal>,
}

impl<T: Terminal> Session<T> {
    /// A task log under `title`.
    #[must_use]
    pub fn task_log(&self, title: impl Into<String>) -> TaskLog<'_, T> {
        TaskLog {
            session: self,
            title: title.into(),
            limit: None,
            sections: vec![Section {
                header: Header::Bare,
                lines: Vec::new(),
            }],
            drawn: None,
            quiet: None,
        }
    }
}

impl<'s, T: Terminal> TaskLog<'s, T> {
    /// Shows only the last `limit` lines of the whole log while it repaints, so
    /// it stays on the screen; 10 by default. Failure still leaves all of them.
    #[must_use]
    pub const fn limit(mut self, limit: NonZeroUsize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Adds a line to the main log; a line break starts another line.
    /// # Errors
    /// The terminal failed.
    pub fn message(&mut self, line: &str) -> Result<()> {
        self.push(0, line)
    }

    /// Opens a group: a name over lines of its own.
    pub fn group(&mut self, name: impl Into<String>) -> TaskLogGroup<'_, 's, T> {
        self.sections.push(Section {
            header: Header::Running(name.into()),
            lines: Vec::new(),
        });
        TaskLogGroup {
            at: self.sections.len() - 1,
            log: self,
        }
    }

    /// Clears the log and leaves a green `◇` line with `message`.
    /// # Errors
    /// The terminal failed.
    pub fn success(self, message: &str) -> Result<()> {
        let skin = self.session.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let line = format!(
            "{}  {message}",
            skin.paint(theme.submitted, symbols.step_submit)
        );
        self.finish(&[line], false)
    }

    /// Clears the log and leaves a red `■` line with `message` over the whole
    /// log.
    /// # Errors
    /// The terminal failed.
    pub fn error(self, message: &str) -> Result<()> {
        let skin = self.session.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let line = format!(
            "{}  {message}",
            skin.paint(theme.cancelled, symbols.step_cancel)
        );
        self.finish(&[line], true)
    }

    fn drawing(&self) -> Drawing {
        match (self.session.mode(), self.session.terminal().kind()) {
            (OutputMode::Accessible, _) => Drawing::Changes,
            (OutputMode::Animated, TerminalKind::Live) => Drawing::Repaint,
            (OutputMode::Animated, TerminalKind::Dumb | TerminalKind::Redirected) => {
                Drawing::End
            }
        }
    }

    fn room(&self) -> Option<usize> {
        self.session
            .terminal()
            .width()
            .map(|width| width.saturating_sub(GUTTER))
    }

    fn head(&self) -> [String; 2] {
        let skin = self.session.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        [
            format!(
                "{}  {}",
                skin.paint(theme.submitted, symbols.step_submit),
                self.title
            ),
            bar(skin),
        ]
    }

    fn section_rows(&self, to_hide: usize) -> Vec<String> {
        let (skin, room) = (self.session.skin(), self.room());
        let mut left = to_hide;
        self.sections
            .iter()
            .flat_map(|section| section.rows(skin, room, &mut left))
            .collect()
    }

    fn push(&mut self, section: usize, line: &str) -> Result<()> {
        let Some(target) = self.sections.get_mut(section) else {
            return Ok(());
        };
        let first = target.lines.len();
        target
            .lines
            .extend(line.split('\n').map(without_cursor_moves));
        self.redraw(Change::Lines(section, first))
    }

    fn redraw(&mut self, change: Change) -> Result<()> {
        match self.drawing() {
            Drawing::End => Ok(()),
            Drawing::Changes => self.write_new(change),
            Drawing::Repaint => self.repaint(),
        }
    }

    fn write_new(&mut self, change: Change) -> Result<()> {
        let (skin, room) = (self.session.skin(), self.room());
        let mut rows: Vec<String> = match self.drawn {
            None => self.head().into(),
            Some(_) => Vec::new(),
        };
        match change {
            Change::Lines(at, first) => {
                if let Some(section) = self.sections.get(at) {
                    if first == 0 {
                        rows.extend(section.header_row(skin));
                    }
                    rows.extend(
                        section
                            .lines
                            .iter()
                            .skip(first)
                            .map(|line| line_row(skin, room, line)),
                    );
                }
            }
            Change::Header(at) => {
                rows.extend(self.sections.get(at).and_then(|s| s.header_row(skin)));
            }
        }
        self.drawn = Some(Vec::new());
        self.session.write(&draw::lines_of(&rows))
    }

    fn repaint(&mut self) -> Result<()> {
        let keep = self.limit.map_or(REPAINTED_LINES, NonZeroUsize::get);
        let lines: usize = self.sections.iter().map(|s| s.lines.len()).sum();
        let rows: Vec<String> = self
            .head()
            .into_iter()
            .chain(self.section_rows(lines.saturating_sub(keep)))
            .collect();
        let hide = if self.quiet.is_some() {
            ""
        } else {
            self.quiet = Some(self.session.terminal().quiet());
            HIDE_CURSOR
        };
        let up = self.rows_on_screen();
        let written = self
            .session
            .write(&format!("{hide}{}", draw::repaint(up, &rows)));
        self.drawn = Some(rows);
        written
    }

    fn rows_on_screen(&self) -> usize {
        draw::screen_rows(
            self.drawn.as_deref().unwrap_or_default(),
            self.session.terminal().width(),
        )
    }

    fn finish(self, last: &[String], log: bool) -> Result<()> {
        let kept = if log && self.drawing() != Drawing::Changes {
            self.section_rows(0)
        } else {
            Vec::new()
        };
        let bar = bar(self.session.skin());
        let spacing = if kept.is_empty() {
            None
        } else {
            Some(bar.clone())
        };
        let rows: Vec<String> = last
            .iter()
            .cloned()
            .chain(spacing)
            .chain(kept)
            .chain([bar])
            .collect();
        match self.drawing() {
            Drawing::Repaint => self
                .session
                .write(&draw::repaint(self.rows_on_screen(), &rows)),
            Drawing::Changes | Drawing::End => {
                self.session.write(&draw::lines_of(&rows))
            }
        }
    }
}

impl<T: Terminal> Drop for TaskLog<'_, T> {
    fn drop(&mut self) {
        if self.quiet.is_some() {
            self.session.write(SHOW_CURSOR).unwrap_or_default();
        }
    }
}

/// A named part of a task log.
#[derive(Debug)]
pub struct TaskLogGroup<'a, 's, T: Terminal> {
    log: &'a mut TaskLog<'s, T>,
    at: usize,
}

impl<T: Terminal> TaskLogGroup<'_, '_, T> {
    /// Adds a line to the group.
    /// # Errors
    /// The terminal failed.
    pub fn message(&mut self, line: &str) -> Result<()> {
        self.log.push(self.at, line)
    }

    /// Marks the group done: its name gives way to a green `◆` and `message`.
    /// # Errors
    /// The terminal failed.
    pub fn success(self, message: impl Into<String>) -> Result<()> {
        self.close(Header::Succeeded(message.into()))
    }

    /// Marks the group failed: its name gives way to a red `■` and `message`.
    /// # Errors
    /// The terminal failed.
    pub fn error(self, message: impl Into<String>) -> Result<()> {
        self.close(Header::Failed(message.into()))
    }

    fn close(self, header: Header) -> Result<()> {
        if let Some(section) = self.log.sections.get_mut(self.at) {
            section.header = header;
        }
        self.log.redraw(Change::Header(self.at))
    }
}

/// What a terminal would show of `line`, as the log may hold it: the text after
/// the last carriage return, tabs spread to the tab stops, and neither escapes
/// that move the cursor or erase nor other control characters, which would
/// wreck the block; colors stay.
fn without_cursor_moves(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut rest = line.rsplit('\r').next().unwrap_or_default();
    let mut kept = String::with_capacity(rest.len());
    // The tab stops are those of the screen, after the rail.
    let mut column = GUTTER;
    while !rest.is_empty() {
        if rest.starts_with('\x1b') {
            let (escape, tail) = rest.split_at(escape_len(rest));
            if escape.starts_with("\x1b[") && escape.ends_with('m') {
                kept.push_str(escape);
            }
            rest = tail;
            continue;
        }
        let (text, tail) = rest.split_at(rest.find('\x1b').unwrap_or(rest.len()));
        for c in text.chars() {
            if c == '\t' {
                let stop = (column / TAB + 1) * TAB;
                kept.extend(std::iter::repeat_n(' ', stop - column));
                column = stop;
            } else if !c.is_control() {
                kept.push(c);
                column += c.width().unwrap_or_default();
            }
        }
        rest = tail;
    }
    kept
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::num::NonZeroUsize;

    use rstest::rstest;

    use crate::{
        core::{
            error::Result,
            session::Session,
            settings::{OutputMode, Settings},
        },
        fake::{Script, Switch, screen},
        output::task_log::without_cursor_moves,
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    fn session(kind: TerminalKind) -> Session<Script> {
        Session::new(Script::new(kind, [], []), Settings::new(ColorChoice::Plain))
    }

    fn last(count: usize) -> NonZeroUsize {
        NonZeroUsize::new(count).unwrap_or(NonZeroUsize::MIN)
    }

    #[test]
    fn only_the_last_lines_stay_on_the_screen() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building").limit(last(2));
        ["one", "two", "three"]
            .into_iter()
            .try_for_each(|line| log.message(line))?;
        assert_eq!(
            screen(&session.terminal().written()),
            "◇  Building\n│\n│  two\n│  three"
        );
        Ok(())
    }

    #[test]
    fn a_title_wider_than_the_terminal_is_erased_whole() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("T".repeat(100));
        log.message("one")?;
        log.message("two")?;
        let raw = session.terminal().written();
        assert!(raw.contains("\x1b[4A"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn ten_lines_stay_unless_told_otherwise() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        (1..=12).try_for_each(|n| log.message(&format!("line {n}")))?;
        let shown = screen(&session.terminal().written());
        assert!(!shown.contains("line 2\n"), "{shown}");
        assert!(shown.ends_with("│  line 12"), "{shown}");
        assert_eq!(shown.lines().count(), 2 + 10, "{shown}");
        log.error("Failed")?;
        let all = screen(&session.terminal().written());
        assert!(all.contains("│  line 1\n"), "{all}");
        Ok(())
    }

    #[test]
    fn success_clears_the_log_and_leaves_one_line() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        log.message("one\ntwo")?;
        log.success("Built")?;
        assert_eq!(screen(&session.terminal().written()), "◇  Built\n│");
        Ok(())
    }

    #[test]
    fn failure_leaves_the_whole_log_under_the_message() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building").limit(last(1));
        log.message("one")?;
        log.message("two")?;
        log.error("Failed")?;
        assert_eq!(
            screen(&session.terminal().written()),
            "■  Failed\n│\n│  one\n│  two\n│"
        );
        Ok(())
    }

    #[test]
    fn groups_have_a_header_and_lines_of_their_own() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        log.message("start")?;
        let mut group = log.group("tests");
        group.message("test one")?;
        group.success("tests passed")?;
        assert_eq!(
            screen(&session.terminal().written()),
            "◇  Building\n│\n│  start\n│  ◆ tests passed\n│  test one"
        );
        Ok(())
    }

    #[test]
    fn a_failed_group_shows_red() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        let group = log.group("tests");
        group.error("tests failed")?;
        let shown = screen(&session.terminal().written());
        assert!(shown.ends_with("│  ■ tests failed"), "{shown}");
        Ok(())
    }

    #[test]
    fn a_terminal_that_is_not_live_gets_only_the_end() -> Result<()> {
        let failing = session(TerminalKind::Redirected);
        let mut log = failing.task_log("Building");
        log.message("one")?;
        assert_eq!(failing.terminal().written(), "");
        log.error("Failed")?;
        assert_eq!(failing.terminal().written(), "■  Failed\n│\n│  one\n│\n");
        let quiet = session(TerminalKind::Redirected);
        let mut ok = quiet.task_log("Building");
        ok.message("one")?;
        ok.success("Built")?;
        assert_eq!(quiet.terminal().written(), "◇  Built\n│\n");
        Ok(())
    }

    #[test]
    fn accessible_output_writes_each_new_row_once() -> Result<()> {
        let session = session(TerminalKind::Live).output(OutputMode::Accessible);
        let mut log = session.task_log("Building").limit(last(1));
        log.message("one")?;
        log.message("two")?;
        assert_eq!(
            session.terminal().written(),
            "◇  Building\n│\n│  one\n│  two\n"
        );
        log.error("Failed")?;
        let raw = session.terminal().written();
        assert!(!raw.contains('\x1b'), "{raw:?}");
        assert!(raw.ends_with("│  two\n■  Failed\n│\n"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn accessible_output_does_not_repeat_rows_a_group_pushed_down() -> Result<()> {
        let session = session(TerminalKind::Live).output(OutputMode::Accessible);
        let mut log = session.task_log("Building");
        log.message("a")?;
        let mut group = log.group("tests");
        group.message("t1")?;
        log.message("b")?;
        assert_eq!(
            session.terminal().written(),
            "◇  Building\n│\n│  a\n│  tests\n│  t1\n│  b\n"
        );
        Ok(())
    }

    #[test]
    fn long_lines_are_cut_to_the_width() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        log.message(&"x".repeat(200))?;
        let shown = screen(&session.terminal().written());
        let widest = shown.lines().map(|line| line.chars().count()).max();
        assert!(widest.is_some_and(|widest| widest <= 80), "{shown}");
        Ok(())
    }

    #[test]
    fn a_repainting_log_keeps_the_terminal_quiet_and_the_cursor_hidden() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        assert_eq!(session.terminal().switches(), []);
        log.message("one")?;
        log.message("two")?;
        assert_eq!(session.terminal().switches(), [Switch::Quieted]);
        let raw = session.terminal().written();
        assert_eq!(raw.matches("\x1b[?25l").count(), 1, "{raw:?}");
        log.success("Built")?;
        assert_eq!(
            session.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        let after = session.terminal().written();
        assert!(after.ends_with("\x1b[?25h"), "{after:?}");
        Ok(())
    }

    #[test]
    fn a_dropped_log_gives_the_terminal_and_the_cursor_back() -> Result<()> {
        let session = session(TerminalKind::Live);
        session.task_log("Building").message("one")?;
        assert_eq!(
            session.terminal().switches(),
            [Switch::Quieted, Switch::Restored]
        );
        assert!(session.terminal().written().ends_with("\x1b[?25h"));
        Ok(())
    }

    #[rstest]
    #[case::accessible(TerminalKind::Live, OutputMode::Accessible)]
    #[case::piped(TerminalKind::Redirected, OutputMode::Animated)]
    fn a_log_that_does_not_repaint_leaves_the_terminal_alone(
        #[case] kind: TerminalKind,
        #[case] output: OutputMode,
    ) -> Result<()> {
        let session = session(kind).output(output);
        let mut log = session.task_log("Building");
        log.message("one")?;
        log.error("Failed")?;
        assert_eq!(session.terminal().switches(), []);
        assert!(!session.terminal().written().contains("\x1b[?25"));
        Ok(())
    }

    #[test]
    fn colored_lines_are_cut_by_their_columns_and_keep_whole_escapes() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        let line = format!("\x1b[1m\x1b[32m   Compiling\x1b[0m {}", "x".repeat(80));
        log.message(&line)?;
        let raw = session.terminal().written();
        let shown = screen(&raw);
        let row = shown.lines().last().unwrap_or_default();
        assert_eq!(row.chars().count(), 80, "{shown}");
        assert!(row.starts_with("│     Compiling x"), "{row}");
        assert!(!raw.contains("\x1b[3\n"), "{raw:?}");
        assert!(raw.contains("\x1b[0m"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn rows_are_erased_at_the_width_the_terminal_has_now() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building");
        log.message(&"x".repeat(70))?;
        session.terminal().resize(40);
        log.message("two")?;
        let raw = session.terminal().written();
        assert!(raw.contains("\x1b[4A"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn the_whole_block_stays_within_the_limit_with_groups() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut log = session.task_log("Building").limit(last(4));
        (1..=3).try_for_each(|n| log.message(&format!("main {n}")))?;
        for name in ["a", "b"] {
            let mut group = log.group(name);
            (1..=3).try_for_each(|n| group.message(&format!("{name} {n}")))?;
        }
        let shown = screen(&session.terminal().written());
        assert_eq!(shown.lines().count(), 2 + 2 + 4, "{shown}");
        assert!(shown.ends_with("│  b 3"), "{shown}");
        assert!(shown.contains("│  a\n│  a 3\n"), "{shown}");
        log.error("Failed")?;
        assert!(screen(&session.terminal().written()).contains("│  main 1\n"));
        Ok(())
    }

    #[test]
    fn a_long_accessible_log_writes_each_row_once() -> Result<()> {
        let session = session(TerminalKind::Live).output(OutputMode::Accessible);
        let mut log = session.task_log("Building");
        (0..2000).try_for_each(|n| log.message(&format!("line {n}")))?;
        let raw = session.terminal().written();
        assert_eq!(raw.lines().count(), 2 + 2000);
        Ok(())
    }

    #[rstest]
    #[case::cursor_up("a\x1b[2Ab", "ab")]
    #[case::erase("a\x1b[2Kb\x1b[Jc", "abc")]
    #[case::color_stays("\x1b[31mred\x1b[0m", "\x1b[31mred\x1b[0m")]
    #[case::plain("plain", "plain")]
    #[case::other_escape("a\x1b7b\x1bMc", "abc")]
    #[case::carriage_returns(
        "layer 1: 10%\rlayer 1: 50%\rlayer 1: 100%",
        "layer 1: 100%"
    )]
    #[case::windows_line_end("text\r", "text")]
    #[case::tab_after_the_rail("a\tb", "a    b")]
    #[case::tab_to_a_stop("abcd\tf", "abcd f")]
    #[case::tab_after_color("\x1b[31mab\x1b[0m\tc", "\x1b[31mab\x1b[0m   c")]
    #[case::control("a\x07b\x08c", "abc")]
    fn cursor_moves_are_dropped_and_colors_stay(
        #[case] line: &str,
        #[case] kept: &str,
    ) {
        assert_eq!(without_cursor_moves(line), kept);
    }
}
