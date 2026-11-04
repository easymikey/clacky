//! The `progress` bar: a spinner whose message starts with a bar.

use std::fmt::Display;

use crate::{
    core::{error::Result, session::Session, settings::OutputMode},
    output::spinner::Spinner,
    render::{color::ColorChoice, draw},
    terminal::Terminal,
};

const SIZE: usize = 40;
const KEPT: usize = 3 + 1 + 12;

/// The character the bar is drawn with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BarStyle {
    /// `─`
    Light,
    /// `━`
    #[default]
    Heavy,
    /// `█`
    Block,
}

impl BarStyle {
    const fn glyph(self) -> &'static str {
        match self {
            Self::Light => "─",
            Self::Heavy => "━",
            Self::Block => "█",
        }
    }
}

/// A bar that fills from `0` to `max`.
///
/// ```no_run
/// # use clacky::{core::session::Session, terminal::Terminal};
/// # fn copy<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// let mut bar = session.progress(3);
/// bar.start("Copying")?;
/// bar.advance(1, "Copying a")?;
/// bar.advance(2, "Copying c")?;
/// bar.stop("Copied")?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct Progress<'s, T: Terminal> {
    session: &'s Session<T>,
    spinner: Spinner<'s, T>,
    style: BarStyle,
    max: u32,
    value: u32,
    message: String,
}

impl<T: Terminal> Session<T> {
    /// A progress bar for `max` steps. With `max` of `0` the bar is full from
    /// the start.
    #[must_use]
    pub fn progress(&self, max: u32) -> Progress<'_, T> {
        Progress {
            session: self,
            spinner: self.spinner(),
            style: BarStyle::default(),
            max,
            value: 0,
            message: String::new(),
        }
    }
}

impl<T: Terminal> Progress<'_, T> {
    /// The bar's character; heavy by default.
    #[must_use]
    pub const fn style(mut self, style: BarStyle) -> Self {
        self.style = style;
        self
    }

    /// Draws the bar at its start with `message` after it.
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn start(&mut self, message: impl Display) -> Result<()> {
        self.message = message.to_string();
        self.spinner.start(self.line())
    }

    /// Moves the bar `step` forward, at most to its end, and shows `message`.
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn advance(&mut self, step: u32, message: impl Display) -> Result<()> {
        self.value = self.value.saturating_add(step).min(self.max);
        self.message = message.to_string();
        self.spinner.message(self.line())
    }

    /// Turns the frame one step; call it every
    /// [`DELAY`](crate::output::spinner::DELAY).
    /// # Errors
    /// The terminal failed, or Ctrl-C came.
    pub fn tick(&mut self) -> Result<()> {
        self.spinner.tick()
    }

    /// Ends with the spinner's green `◇` line and `message`.
    /// # Errors
    /// The terminal failed, or Ctrl-C came before the end.
    pub fn stop(&mut self, message: impl Display) -> Result<()> {
        self.spinner.stop(message)
    }

    /// Ends with a red `■` line.
    /// # Errors
    /// The terminal failed.
    pub fn cancel(&mut self, message: impl Display) -> Result<()> {
        self.spinner.cancel(message)
    }

    /// Ends with a red `▲` line.
    /// # Errors
    /// The terminal failed.
    pub fn error(&mut self, message: impl Display) -> Result<()> {
        self.spinner.error(message)
    }

    fn line(&self) -> String {
        let room = self
            .session
            .terminal()
            .width()
            .map(|width| width.saturating_sub(SIZE + KEPT));
        let ellipsis = self.session.skin().symbols().ellipsis;
        let message = self.message.replace('\n', " ");
        let message = match room {
            Some(room) => draw::clip_row(&message, room, ellipsis),
            None => message,
        };
        format!("{} {message}", self.bar())
    }

    /// The bar, or `value/max` where the bar would show nothing: with no color
    /// the filled and the empty part look alike, and a screen reader would read
    /// 40 glyphs.
    fn bar(&self) -> String {
        let skin = self.session.skin();
        match (skin.color(), self.spinner.mode()) {
            (ColorChoice::Colored, OutputMode::Animated) => {}
            (ColorChoice::Plain, _) | (_, OutputMode::Accessible) => {
                return format!("{}/{}", self.value, self.max);
            }
        }
        let theme = skin.theme();
        let filled = filled(self.value, self.max);
        let glyph = self.style.glyph();
        format!(
            "{}{}",
            skin.paint(theme.busy, &glyph.repeat(filled)),
            skin.paint(theme.rail, &glyph.repeat(SIZE - filled))
        )
    }
}

fn filled(value: u32, max: u32) -> usize {
    if max == 0 {
        return SIZE;
    }
    let max = u64::from(max);
    let value = u64::from(value).min(max);
    let size = u64::try_from(SIZE).unwrap_or_default();
    usize::try_from((value * size + max / 2) / max)
        .unwrap_or(SIZE)
        .min(SIZE)
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use crate::{
        core::{
            error::Result,
            session::Session,
            settings::{OutputMode, Settings},
        },
        fake::{Script, screen, without_escapes},
        output::progress::{BarStyle, filled},
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    fn session(kind: TerminalKind) -> Session<Script> {
        Session::new(Script::new(kind, [], []), Settings::new(ColorChoice::Plain))
    }

    fn colored(kind: TerminalKind) -> Session<Script> {
        Session::new(
            Script::new(kind, [], []),
            Settings::new(ColorChoice::Colored),
        )
    }

    #[rstest]
    #[case::empty(0, 10, 0)]
    #[case::quarter(5, 20, 10)]
    #[case::rounds_up(1, 3, 13)]
    #[case::full(10, 10, 40)]
    #[case::past_the_end(99, 10, 40)]
    #[case::no_steps(0, 0, 40)]
    #[case::no_steps_past(5, 0, 40)]
    fn the_bar_fills_in_proportion(
        #[case] value: u32,
        #[case] max: u32,
        #[case] columns: usize,
    ) {
        assert_eq!(filled(value, max), columns);
    }

    #[test]
    fn accessible_output_writes_a_line_per_step_without_cursor_moves() -> Result<()> {
        let session = session(TerminalKind::Live).output(OutputMode::Accessible);
        let mut bar = session.progress(2);
        bar.start("Copying")?;
        bar.tick()?;
        bar.advance(1, "Copying a")?;
        bar.stop("Copied")?;
        let raw = session.terminal().written();
        assert!(!raw.contains('\x1b'), "{raw:?}");
        assert_eq!(raw.lines().count(), 4, "{raw:?}");
        assert!(raw.ends_with("◇  Copied\n"), "{raw:?}");
        Ok(())
    }

    #[rstest]
    #[case::light(BarStyle::Light, '─')]
    #[case::heavy(BarStyle::Heavy, '━')]
    #[case::block(BarStyle::Block, '█')]
    fn each_style_draws_its_own_character(
        #[case] style: BarStyle,
        #[case] glyph: char,
    ) -> Result<()> {
        let session = colored(TerminalKind::Live);
        let mut bar = session.progress(4).style(style);
        bar.start("Copying")?;
        bar.advance(1, "Copying a")?;
        let shown = screen(&session.terminal().written());
        let expected = format!("│\n◒  {} Copying a", glyph.to_string().repeat(40));
        assert_eq!(shown, expected);
        Ok(())
    }

    #[test]
    fn the_bar_ends_with_the_spinners_done_line() -> Result<()> {
        let session = colored(TerminalKind::Live);
        let mut bar = session.progress(2);
        bar.start("Copying")?;
        bar.advance(1, "Copying a")?;
        bar.tick()?;
        bar.advance(5, "Copying b")?;
        let full = screen(&session.terminal().written());
        assert_eq!(full, format!("│\n◐  {} Copying b", "━".repeat(40)));
        bar.stop("Copied")?;
        assert_eq!(screen(&session.terminal().written()), "│\n◇  Copied");
        Ok(())
    }

    #[test]
    fn a_piped_bar_writes_a_line_per_step() -> Result<()> {
        let session = session(TerminalKind::Redirected);
        let mut bar = session.progress(0);
        bar.start("Copying")?;
        bar.advance(1, "Copying a")?;
        bar.stop("Copied")?;
        let raw = session.terminal().written();
        assert_eq!(raw.lines().count(), 4, "{raw:?}");
        assert!(raw.ends_with("◇  Copied\n"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn without_color_the_bar_is_a_count() -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut bar = session.progress(4);
        bar.start("Copying")?;
        bar.advance(1, "Copying a")?;
        assert_eq!(screen(&session.terminal().written()), "│\n◒  1/4 Copying a");
        bar.advance(1, "Copying b")?;
        assert_eq!(screen(&session.terminal().written()), "│\n◒  2/4 Copying b");
        Ok(())
    }

    #[test]
    fn accessible_output_counts_even_with_color() -> Result<()> {
        let session = colored(TerminalKind::Live).output(OutputMode::Accessible);
        let mut bar = session.progress(4);
        bar.start("Copying")?;
        bar.advance(2, "Copying b")?;
        let raw = session.terminal().written();
        let plain = without_escapes(&raw);
        assert!(plain.ends_with("◒  2/4 Copying b...\n"), "{plain:?}");
        assert!(!plain.contains('━'), "{plain:?}");
        Ok(())
    }

    #[rstest]
    #[case::cancel(false, "■  Nope")]
    #[case::error(true, "▲  Nope")]
    fn a_bar_can_end_in_red(#[case] error: bool, #[case] line: &str) -> Result<()> {
        let session = session(TerminalKind::Live);
        let mut bar = session.progress(2);
        bar.start("Copying")?;
        if error {
            bar.error("Nope")?;
        } else {
            bar.cancel("Nope")?;
        }
        assert_eq!(screen(&session.terminal().written()), format!("│\n{line}"));
        Ok(())
    }
}
