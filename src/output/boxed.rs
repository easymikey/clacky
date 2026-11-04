//! A framed block of text on the rail.

use crate::{
    core::{error::Result, session::Session},
    render::{
        color::measure_text_width,
        draw,
        style::Skin,
        symbols::Symbols,
        wrap::wrap,
    },
    terminal::Terminal,
};

const DEFAULT_COLUMNS: usize = 80;
const GUIDE: usize = 2;
const SMALLEST: usize = 3;

/// Where text sits between the borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Against the left border.
    Left,
    /// In the middle.
    Center,
    /// Against the right border.
    Right,
}

/// How wide the frame is, borders included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxWidth {
    /// As wide as the text and the title need, up to the terminal.
    Auto,
    /// This many columns, or the terminal's room if that is less.
    Fixed(u16),
    /// `Fraction(numerator, denominator)` of the room the terminal leaves.
    Fraction(u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The corner glyphs of a box.
pub enum Corners {
    /// Right-angled corners.
    Square,
    /// Rounded corners.
    Rounded,
}

impl Corners {
    const fn glyphs(self, symbols: &Symbols) -> [&'static str; 4] {
        match self {
            Self::Square => [
                symbols.bar_start,
                symbols.bar_start_right,
                symbols.bar_end,
                symbols.bar_end_right,
            ],
            Self::Rounded => [
                symbols.corner_top_left,
                symbols.corner_top_right,
                symbols.corner_bottom_left,
                symbols.corner_bottom_right,
            ],
        }
    }
}

/// A frame with a title and wrapped text; build it with [`Session::boxed`].
///
/// ```no_run
/// # use clacky::{
/// #     output::boxed::{Align, BoxWidth, Corners},
/// #     core::session::Session,
/// #     terminal::Terminal,
/// # };
/// # fn show<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// session
///     .boxed("Saved", "Your settings are in `~/.config/acme`.")
///     .width(BoxWidth::Fraction(1, 2))
///     .content_align(Align::Center)
///     .corners(Corners::Square)
///     .show()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Boxed<'s, T: Terminal> {
    session: &'s Session<T>,
    title: String,
    body: String,
    width: BoxWidth,
    title_align: Align,
    content_align: Align,
    corners: Corners,
    padding: u16,
    title_padding: u16,
}

impl<T: Terminal> Session<T> {
    /// Starts a framed block with rounded corners, as in `note`. The title may
    /// be empty.
    pub fn boxed(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Boxed<'_, T> {
        // Clack's docs promise rounded corners and a plain width, but its code
        // draws square corners and reads a number as a fraction. We follow the
        // docs.
        Boxed {
            session: self,
            title: title.into(),
            body: body.into(),
            width: BoxWidth::Auto,
            title_align: Align::Left,
            content_align: Align::Left,
            corners: Corners::Rounded,
            padding: 2,
            title_padding: 1,
        }
    }
}

impl<T: Terminal> Boxed<'_, T> {
    /// How wide the frame is.
    #[must_use]
    pub fn width(self, width: BoxWidth) -> Self {
        Self { width, ..self }
    }

    /// Where the title sits.
    #[must_use]
    pub fn title_align(self, align: Align) -> Self {
        Self {
            title_align: align,
            ..self
        }
    }

    /// Where the lines sit.
    #[must_use]
    pub fn content_align(self, align: Align) -> Self {
        Self {
            content_align: align,
            ..self
        }
    }

    /// Which corners the frame has.
    #[must_use]
    pub fn corners(self, corners: Corners) -> Self {
        Self { corners, ..self }
    }

    /// Spaces between the side borders and the text; two by default.
    #[must_use]
    pub fn padding(self, columns: u16) -> Self {
        Self {
            padding: columns,
            ..self
        }
    }

    /// Spaces around the title; one by default.
    #[must_use]
    pub fn title_padding(self, columns: u16) -> Self {
        Self {
            title_padding: columns,
            ..self
        }
    }

    /// Writes the frame and a rail row under it.
    /// # Errors
    /// The terminal failed.
    pub fn show(self) -> Result<()> {
        let columns = self.session.terminal().width().unwrap_or(DEFAULT_COLUMNS);
        self.session
            .write(&self.render(self.session.skin(), columns))
    }

    fn render(&self, skin: &Skin, columns: usize) -> String {
        let room = columns.saturating_sub(GUIDE).max(SMALLEST);
        let outer = match self.width {
            BoxWidth::Auto => None,
            BoxWidth::Fixed(width) => Some(usize::from(width)),
            BoxWidth::Fraction(_, 0) => Some(room),
            BoxWidth::Fraction(top, bottom) => {
                Some(room * usize::from(top) / usize::from(bottom))
            }
        };
        let inner_cap = outer.unwrap_or(room).min(room).max(SMALLEST) - 2;
        let pad = usize::from(self.padding).min(inner_cap.saturating_sub(1) / 2);
        let lines = wrap(&self.body, (inner_cap - 2 * pad).max(1), "");
        let title_pad = if self.title.is_empty() {
            0
        } else {
            usize::from(self.title_padding)
        };
        let inner = match outer {
            Some(_) => inner_cap,
            None => lines
                .iter()
                .map(|line| measure_text_width(line) + 2 * pad)
                .chain([measure_text_width(&self.title) + 2 * title_pad])
                .max()
                .unwrap_or_default()
                .clamp(1, inner_cap),
        };
        let (theme, symbols) = (skin.theme(), skin.symbols());
        let [top_left, top_right, bottom_left, bottom_right] =
            self.corners.glyphs(skin.drawn());
        let side = skin.paint(theme.rail, skin.drawn().bar);
        let guide = format!("{} ", skin.paint(theme.rail, symbols.bar));
        let dashes =
            |count: usize| skin.paint(theme.rail, &symbols.bar_h.repeat(count));
        let title = fit_title(
            &self.title,
            inner.saturating_sub(2 * title_pad),
            symbols.ellipsis,
        );
        let title = match title.as_str() {
            "" => String::new(),
            title => {
                format!("{0}{title}{0}", " ".repeat(title_pad))
            }
        };
        let spare = inner.saturating_sub(measure_text_width(&title));
        let (lead, trail) = split_spare(spare, self.title_align);
        let top = format!(
            "{guide}{}{}{title}{}{}\n",
            skin.paint(theme.rail, top_left),
            dashes(lead),
            dashes(trail),
            skin.paint(theme.rail, top_right)
        );
        let text_width = inner.saturating_sub(2 * pad);
        let rows = lines
            .iter()
            .map(|line| {
                let (before, after) = split_spare(
                    text_width.saturating_sub(measure_text_width(line)),
                    self.content_align,
                );
                format!(
                    "{guide}{side}{}{}{line}{}{}{side}\n",
                    " ".repeat(pad),
                    " ".repeat(before),
                    " ".repeat(after),
                    " ".repeat(pad),
                )
            })
            .collect::<Vec<_>>()
            .concat();
        let bottom = format!(
            "{guide}{}{}{}\n",
            skin.paint(theme.rail, bottom_left),
            dashes(inner),
            skin.paint(theme.rail, bottom_right)
        );
        format!(
            "{top}{rows}{bottom}{}\n",
            skin.paint(theme.rail, symbols.bar)
        )
    }
}

const fn split_spare(spare: usize, align: Align) -> (usize, usize) {
    match align {
        Align::Left => (0, spare),
        Align::Center => (spare / 2, spare - spare / 2),
        Align::Right => (spare, 0),
    }
}

fn fit_title(title: &str, room: usize, ellipsis: &str) -> String {
    match room {
        0 => String::new(),
        room => draw::clip_row(title, room, ellipsis),
    }
}

#[cfg(test)]
mod tests {
    use super::{Align, BoxWidth, Boxed, Corners};
    use crate::{
        core::{session::Session, settings::Settings},
        fake::{Script, screen},
        render::{color::ColorChoice, style::Skin},
        terminal::TerminalKind,
    };

    fn session() -> Session<Script> {
        Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        )
    }

    fn drawn(frame: &Boxed<'_, Script>, columns: usize) -> String {
        screen(&frame.render(&Skin::new(ColorChoice::Plain), columns))
    }

    #[test]
    fn spaces_and_styles_survive_the_frame() {
        let session = session();
        let frame = session.boxed("", "a  b");
        assert!(drawn(&frame, 40).contains("a  b"));
    }

    #[test]
    fn auto_width_fits_the_text() {
        let session = session();
        assert_eq!(
            drawn(&session.boxed("Saved", "done"), 40),
            "│ ╭ Saved ─╮\n\
             │ │  done  │\n\
             │ ╰────────╯\n\
             │"
        );
    }

    #[test]
    fn fixed_square_right_aligned() {
        let session = session();
        let frame = session
            .boxed("T", "hi")
            .width(BoxWidth::Fixed(12))
            .corners(Corners::Square)
            .content_align(Align::Right)
            .title_align(Align::Center);
        assert_eq!(
            drawn(&frame, 40),
            "│ ┌─── T ────┐\n\
             │ │      hi  │\n\
             │ └──────────┘\n\
             │"
        );
    }

    #[test]
    fn narrow_terminal_wraps_and_truncates_the_title() {
        let session = session();
        let frame = session
            .boxed("A very long title", "one two three four")
            .padding(1);
        let shown = drawn(&frame, 14);
        assert!(
            shown.lines().all(|line| line.chars().count() <= 14),
            "{shown}"
        );
        assert!(shown.contains('…'), "{shown}");
        assert!(shown.contains("three"), "{shown}");
    }

    #[test]
    fn fraction_takes_part_of_the_room() {
        let session = session();
        let frame = session.boxed("", "x").width(BoxWidth::Fraction(1, 2));
        assert_eq!(
            drawn(&frame, 42)
                .lines()
                .next()
                .map(|row| row.chars().count()),
            Some(22)
        );
    }

    #[test]
    fn show_writes_through_the_session() {
        let session = session();
        let shown = session.boxed("Hi", "there").show();
        assert!(shown.is_ok());
        assert!(screen(&session.terminal().written()).contains("│ │  there  │"));
    }
}
