//! How text is painted: colors, styles, the theme that names a style per role,
//! and the skin that paints with a theme and a set of symbols.

use std::{env, ffi::OsStr, fmt::Write as _};

use crate::render::{
    color::ColorChoice,
    messages::Messages,
    settings::settings,
    symbols::{Guide, Symbols},
};

/// The eight colors every terminal has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseColor {
    /// Black.
    Black,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Blue.
    Blue,
    /// Magenta.
    Magenta,
    /// Cyan.
    Cyan,
    /// White.
    White,
}

impl BaseColor {
    const fn index(self) -> u8 {
        match self {
            Self::Black => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
            Self::White => 7,
        }
    }
}

/// A color a style paints text or its background with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Black.
    Black,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Blue.
    Blue,
    /// Magenta.
    Magenta,
    /// Cyan.
    Cyan,
    /// White.
    White,
    /// The bright variant of a base color.
    Bright(BaseColor),
    /// A place in the 256 color palette.
    Ansi256(u8),
    /// A true color; drawn as the nearest palette color where the terminal does
    /// not announce [`ColorDepth::TrueColor`].
    Rgb(u8, u8, u8),
}

/// How many colors the terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// The 16 colors of the base palette.
    Basic,
    /// The 256 color palette.
    Ansi256,
    /// 24 bit colors.
    TrueColor,
}

impl ColorDepth {
    /// What `COLORTERM` and `TERM` of this process say.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(
            env::var_os("COLORTERM").as_deref(),
            env::var_os("TERM").as_deref(),
        )
    }

    /// The depth the values of `COLORTERM` and `TERM` announce.
    #[must_use]
    pub fn from_vars(colorterm: Option<&OsStr>, term: Option<&OsStr>) -> Self {
        if colorterm.is_some_and(|value| value == "truecolor" || value == "24bit") {
            Self::TrueColor
        } else if term
            .is_some_and(|value| value.as_encoded_bytes().ends_with(b"256color"))
        {
            Self::Ansi256
        } else {
            Self::Basic
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    /// A place in the 16 color palette.
    Basic(u8),
    Indexed(u8),
    True(u8, u8, u8),
}

const PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
const GREY_FIRST: u8 = 232;

fn distance((r, g, b): (u8, u8, u8), (to_r, to_g, to_b): (u8, u8, u8)) -> u32 {
    [r.abs_diff(to_r), g.abs_diff(to_g), b.abs_diff(to_b)]
        .into_iter()
        .map(|step| u32::from(step).pow(2))
        .sum()
}

fn nearest_basic(rgb: (u8, u8, u8)) -> u8 {
    PALETTE
        .iter()
        .zip(0..)
        .min_by_key(|(color, _)| distance(rgb, **color))
        .map_or(0, |(_, place)| place)
}

pub(crate) fn nearest_ansi256((r, g, b): (u8, u8, u8)) -> u8 {
    let level = |value: u8| -> u8 {
        CUBE.iter()
            .zip(0..)
            .min_by_key(|(step, _)| step.abs_diff(value))
            .map_or(0, |(_, place)| place)
    };
    let cube = (level(r), level(g), level(b));
    let cube_rgb = {
        let shade =
            |place: u8| CUBE.get(usize::from(place)).copied().unwrap_or_default();
        (shade(cube.0), shade(cube.1), shade(cube.2))
    };
    let grey_place = (u8::try_from(
        (u32::from(r) + u32::from(g) + u32::from(b)).saturating_sub(9) / 30,
    )
    .unwrap_or(u8::MAX))
    .min(23);
    let grey = grey_place * 10 + 8;
    let cube_place = 16 + 36 * cube.0 + 6 * cube.1 + cube.2;
    if distance((r, g, b), (grey, grey, grey)) < distance((r, g, b), cube_rgb) {
        GREY_FIRST + grey_place
    } else {
        cube_place
    }
}

fn palette_rgb(place: u8) -> (u8, u8, u8) {
    let shade = |step: u8| CUBE.get(usize::from(step)).copied().unwrap_or_default();
    match place {
        0..16 => PALETTE.get(usize::from(place)).copied().unwrap_or_default(),
        16..GREY_FIRST => {
            let cube = place - 16;
            (shade(cube / 36), shade(cube / 6 % 6), shade(cube % 6))
        }
        GREY_FIRST.. => {
            let grey = (place - GREY_FIRST) * 10 + 8;
            (grey, grey, grey)
        }
    }
}

impl Color {
    fn resolve(self, depth: ColorDepth) -> Resolved {
        match (self, depth) {
            (Self::Black, _) => Resolved::Basic(0),
            (Self::Red, _) => Resolved::Basic(1),
            (Self::Green, _) => Resolved::Basic(2),
            (Self::Yellow, _) => Resolved::Basic(3),
            (Self::Blue, _) => Resolved::Basic(4),
            (Self::Magenta, _) => Resolved::Basic(5),
            (Self::Cyan, _) => Resolved::Basic(6),
            (Self::White, _) => Resolved::Basic(7),
            (Self::Bright(base), _) => Resolved::Basic(8 + base.index()),
            (Self::Ansi256(place @ 0..16), _) => Resolved::Basic(place),
            (Self::Ansi256(place), ColorDepth::Basic) => {
                Resolved::Basic(nearest_basic(palette_rgb(place)))
            }
            (Self::Ansi256(place), ColorDepth::Ansi256 | ColorDepth::TrueColor) => {
                Resolved::Indexed(place)
            }
            (Self::Rgb(r, g, b), ColorDepth::TrueColor) => Resolved::True(r, g, b),
            (Self::Rgb(r, g, b), ColorDepth::Ansi256) => {
                Resolved::Indexed(nearest_ansi256((r, g, b)))
            }
            (Self::Rgb(r, g, b), ColorDepth::Basic) => {
                Resolved::Basic(nearest_basic((r, g, b)))
            }
        }
    }
}

const FOREGROUND: u8 = 30;
const BRIGHT_FOREGROUND: u8 = 90;
const BACKGROUND_SHIFT: u8 = 10;
const EXTENDED_FOREGROUND: u8 = 38;

fn write_color(out: &mut String, color: Color, depth: ColorDepth, shift: u8) {
    let written = match color.resolve(depth) {
        Resolved::Basic(place @ 0..8) => {
            write!(out, "\x1b[{}m", FOREGROUND + shift + place)
        }
        Resolved::Basic(place) => {
            write!(out, "\x1b[{}m", BRIGHT_FOREGROUND + shift + place - 8)
        }
        Resolved::Indexed(place) => {
            write!(out, "\x1b[{};5;{place}m", EXTENDED_FOREGROUND + shift)
        }
        Resolved::True(r, g, b) => {
            write!(out, "\x1b[{};2;{r};{g};{b}m", EXTENDED_FOREGROUND + shift)
        }
    };
    written.unwrap_or_default();
}

const BOLD: u8 = 1;
const DIM: u8 = 1 << 1;
const ITALIC: u8 = 1 << 2;
const UNDERLINE: u8 = 1 << 3;
const REVERSE: u8 = 1 << 4;
const STRIKE: u8 = 1 << 5;
const ATTRIBUTES: [(u8, u8); 6] = [
    (BOLD, 1),
    (DIM, 2),
    (ITALIC, 3),
    (UNDERLINE, 4),
    (REVERSE, 7),
    (STRIKE, 9),
];

/// A color and attributes for a piece of text.
///
/// ```
/// use clacky::render::style::{Color, Style};
/// let orange = Style::new().fg(Color::Rgb(255, 120, 0)).bold();
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    fg: Option<Color>,
    bg: Option<Color>,
    attributes: u8,
}

impl Style {
    /// A style that paints nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            attributes: 0,
        }
    }

    /// Sets the text color.
    #[must_use]
    pub const fn fg(self, color: Color) -> Self {
        Self {
            fg: Some(color),
            ..self
        }
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(self, color: Color) -> Self {
        Self {
            bg: Some(color),
            ..self
        }
    }

    /// Makes the text bold.
    #[must_use]
    pub const fn bold(self) -> Self {
        self.with(BOLD)
    }

    /// Makes the text dim.
    #[must_use]
    pub const fn dim(self) -> Self {
        self.with(DIM)
    }

    /// Makes the text italic.
    #[must_use]
    pub const fn italic(self) -> Self {
        self.with(ITALIC)
    }

    /// Underlines the text.
    #[must_use]
    pub const fn underline(self) -> Self {
        self.with(UNDERLINE)
    }

    /// Strikes the text through.
    #[must_use]
    pub const fn strike(self) -> Self {
        self.with(STRIKE)
    }

    /// Swaps text and background colors.
    #[must_use]
    pub const fn reverse(self) -> Self {
        self.with(REVERSE)
    }

    const fn with(self, attribute: u8) -> Self {
        Self {
            attributes: self.attributes | attribute,
            ..self
        }
    }

    /// `text` in this style: the foreground, the background, then the
    /// attributes, each as its own escape, and a reset after the text.
    #[must_use]
    pub fn paint(self, color: ColorChoice, depth: ColorDepth, text: &str) -> String {
        const RESET: &str = "\x1b[0m";
        if color == ColorChoice::Plain || self == Self::new() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len() + 24);
        if let Some(fg) = self.fg {
            write_color(&mut out, fg, depth, 0);
        }
        if let Some(bg) = self.bg {
            write_color(&mut out, bg, depth, BACKGROUND_SHIFT);
        }
        for (_, code) in ATTRIBUTES
            .iter()
            .filter(|(attribute, _)| self.attributes & attribute != 0)
        {
            write!(out, "\x1b[{code}m").unwrap_or_default();
        }
        out.push_str(text);
        out.push_str(RESET);
        out
    }
}

settings! {
    /// One style per role the library paints. `Theme::clack()` is the look of
    /// clack; change a role with its builder method.
    ///
    /// ```
    /// use clacky::render::style::{Color, Style, Theme};
    /// let theme = Theme::clack()
    ///     .active(Style::new().fg(Color::Rgb(255, 120, 0)).bold())
    ///     .hint(Style::new().dim());
    /// assert_eq!(theme.hint, Style::new().dim());
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct Theme {
        /// The rail, borders and what is not chosen.
        rail: Style,
        /// The open question: its marker and rail.
        active: Style,
        /// The marker of an answered question.
        submitted: Style,
        /// The marker of a question backed out of.
        cancelled: Style,
        /// A refused answer: the marker, rail and text of the warning.
        warning: Style,
        /// A failure.
        error: Style,
        /// Hints after an option, counts, `…`, and secondary text.
        hint: Style,
        /// The text shown until the first key.
        placeholder: Style,
        /// The option or box that is picked.
        selected: Style,
        /// The option or box that is not picked.
        unselected: Style,
        /// Information log entries.
        info: Style,
        /// Success log entries.
        success: Style,
        /// Step log entries.
        step: Style,
        /// The title in the opening line.
        title: Style,
        /// Work in progress: the spinner and the filled part of a bar.
        busy: Style,
        /// The answer of a question backed out of.
        struck: Style,
        /// An option that cannot be picked.
        disabled: Style,
        /// Text between backticks.
        code: Style,
        /// The caret, and the part of a date being edited.
        cursor: Style,
    }
}

impl Theme {
    /// The colors of clack.
    #[must_use]
    pub const fn clack() -> Self {
        let dim = Style::new().dim();
        Self {
            rail: dim,
            active: Style::new().fg(Color::Cyan),
            submitted: Style::new().fg(Color::Green),
            cancelled: Style::new().fg(Color::Red),
            warning: Style::new().fg(Color::Yellow),
            error: Style::new().fg(Color::Red),
            hint: dim,
            placeholder: dim,
            selected: Style::new().fg(Color::Green),
            unselected: dim,
            info: Style::new().fg(Color::Blue),
            success: Style::new().fg(Color::Green),
            step: Style::new().fg(Color::Green),
            title: Style::new().reverse(),
            busy: Style::new().fg(Color::Magenta),
            struck: dim.strike(),
            disabled: dim.strike(),
            code: Style::new().fg(Color::Cyan),
            cursor: Style::new().reverse(),
        }
    }

    /// Attributes only, no colors: for terminals that show bold and dim but not
    /// color.
    #[must_use]
    pub const fn mono() -> Self {
        let dim = Style::new().dim();
        let bold = Style::new().bold();
        Self {
            rail: dim,
            active: bold,
            submitted: Style::new(),
            cancelled: Style::new().strike(),
            warning: bold,
            error: bold,
            hint: dim,
            placeholder: dim,
            selected: bold,
            unselected: dim,
            info: Style::new(),
            success: Style::new(),
            step: Style::new(),
            title: Style::new().reverse(),
            busy: Style::new(),
            struck: dim.strike(),
            disabled: dim.strike(),
            code: bold,
            cursor: Style::new().reverse(),
        }
    }
}

/// Everything that decides how text looks: whether it is colored, how deep the
/// colors can go, the theme, the symbols, the rail and the texts.
#[derive(Debug, Clone)]
pub struct Skin {
    color: ColorChoice,
    depth: ColorDepth,
    theme: Theme,
    symbols: Symbols,
    railless: Option<Symbols>,
    messages: Messages,
}

impl Skin {
    /// The clack theme and the unicode symbols, with colors as deep as they go.
    #[must_use]
    pub const fn new(color: ColorChoice) -> Self {
        Self {
            color,
            depth: ColorDepth::TrueColor,
            theme: Theme::clack(),
            symbols: Symbols::unicode(),
            railless: None,
            messages: Messages::english(),
        }
    }

    /// Sets how many colors the terminal shows.
    #[must_use]
    pub fn with_depth(self, depth: ColorDepth) -> Self {
        Self { depth, ..self }
    }

    /// Sets the theme.
    #[must_use]
    pub fn with_theme(self, theme: Theme) -> Self {
        Self { theme, ..self }
    }

    /// Sets the glyphs.
    #[must_use]
    pub fn with_symbols(self, symbols: Symbols) -> Self {
        let railless = self.railless.map(|_| symbols.without_rail());
        Self {
            symbols,
            railless,
            ..self
        }
    }

    #[must_use]
    pub(crate) fn with_guide(self, guide: Guide) -> Self {
        let railless = match guide {
            Guide::Drawn => None,
            Guide::Hidden => Some(self.symbols.without_rail()),
        };
        Self { railless, ..self }
    }

    /// Sets the texts.
    #[must_use]
    pub fn with_messages(self, messages: Messages) -> Self {
        Self { messages, ..self }
    }

    /// Whether output is colored.
    #[must_use]
    pub const fn color(&self) -> ColorChoice {
        self.color
    }

    /// The theme.
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The glyphs, without the rail when the guide is hidden.
    #[must_use]
    pub const fn symbols(&self) -> &Symbols {
        match &self.railless {
            Some(railless) => railless,
            None => &self.symbols,
        }
    }

    /// The symbols as chosen, with the rail, for a box that keeps its sides
    /// when the guide is hidden.
    #[must_use]
    pub const fn drawn(&self) -> &Symbols {
        &self.symbols
    }

    #[must_use]
    pub(crate) const fn guide(&self) -> Guide {
        match self.railless {
            Some(_) => Guide::Hidden,
            None => Guide::Drawn,
        }
    }

    /// The texts.
    #[must_use]
    pub const fn messages(&self) -> &Messages {
        &self.messages
    }

    /// `text` in `style`; plain when the output takes no color.
    #[must_use]
    pub fn paint(&self, style: Style, text: &str) -> String {
        style.paint(self.color, self.depth, text)
    }

    /// `text` with what sits between backticks in the code style, the backticks
    /// gone; plain text keeps them.
    #[must_use]
    pub fn code(&self, text: &str) -> String {
        self.code_on(text, str::to_owned)
    }

    /// Like [`code`](Self::code), with the rest of the text made by `rest`.
    #[must_use]
    pub fn code_on(&self, text: &str, rest: impl Fn(&str) -> String) -> String {
        match self.color {
            ColorChoice::Colored => text
                .split('`')
                .enumerate()
                .map(|(at, part)| match at % 2 {
                    1 => self.paint(self.theme.code, part),
                    _ => rest(part),
                })
                .collect(),
            ColorChoice::Plain => rest(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use rstest::rstest;

    use crate::render::{
        color::ColorChoice,
        style::{BaseColor, Color, ColorDepth, Skin, Style, Theme, nearest_ansi256},
    };

    const TRUE: ColorDepth = ColorDepth::TrueColor;

    #[rstest]
    #[case::dim(Style::new().dim(), "\x1b[2mx\x1b[0m")]
    #[case::cyan(Style::new().fg(Color::Cyan), "\x1b[36mx\x1b[0m")]
    #[case::cyan_bold(Style::new().fg(Color::Cyan).bold(), "\x1b[36m\x1b[1mx\x1b[0m")]
    #[case::struck(Style::new().dim().strike(), "\x1b[2m\x1b[9mx\x1b[0m")]
    #[case::reverse(Style::new().reverse(), "\x1b[7mx\x1b[0m")]
    #[case::bright(Style::new().fg(Color::Bright(BaseColor::Red)), "\x1b[91mx\x1b[0m")]
    #[case::background(Style::new().bg(Color::Blue).italic(), "\x1b[44m\x1b[3mx\x1b[0m")]
    #[case::bright_background(
        Style::new().bg(Color::Bright(BaseColor::White)).underline(),
        "\x1b[107m\x1b[4mx\x1b[0m"
    )]
    #[case::palette(Style::new().fg(Color::Ansi256(208)), "\x1b[38;5;208mx\x1b[0m")]
    #[case::true_color(
        Style::new().fg(Color::Rgb(255, 120, 0)),
        "\x1b[38;2;255;120;0mx\x1b[0m"
    )]
    #[case::empty(Style::new(), "x")]
    fn colored_text_is_wrapped_in_sgr(#[case] style: Style, #[case] expected: &str) {
        assert_eq!(style.paint(ColorChoice::Colored, TRUE, "x"), expected);
    }

    #[test]
    fn plain_paints_nothing_with_any_style() {
        let loud = Style::new().fg(Color::Rgb(1, 2, 3)).bg(Color::Red).bold();
        assert_eq!(loud.paint(ColorChoice::Plain, TRUE, "x"), "x");
    }

    #[rstest]
    #[case::grey(Color::Rgb(128, 128, 128), "\x1b[38;5;244m")]
    #[case::orange(Color::Rgb(255, 120, 0), "\x1b[38;5;208m")]
    #[case::white(Color::Rgb(255, 255, 255), "\x1b[38;5;231m")]
    #[case::black(Color::Rgb(0, 0, 0), "\x1b[38;5;16m")]
    fn true_color_falls_back_to_the_palette(#[case] color: Color, #[case] sgr: &str) {
        let painted = Style::new().fg(color).paint(
            ColorChoice::Colored,
            ColorDepth::Ansi256,
            "x",
        );
        assert_eq!(painted, format!("{sgr}x\x1b[0m"));
    }

    #[rstest]
    #[case::red(Color::Rgb(250, 10, 10), "\x1b[91m")]
    #[case::dark_blue(Color::Rgb(0, 0, 200), "\x1b[34m")]
    #[case::palette_orange(Color::Ansi256(208), "\x1b[33m")]
    #[case::palette_base(Color::Ansi256(2), "\x1b[32m")]
    fn basic_terminals_get_the_sixteen_colors(#[case] color: Color, #[case] sgr: &str) {
        let painted =
            Style::new()
                .fg(color)
                .paint(ColorChoice::Colored, ColorDepth::Basic, "x");
        assert_eq!(painted, format!("{sgr}x\x1b[0m"));
    }

    #[test]
    fn ansi256_is_kept_where_the_terminal_has_it() {
        let style = Style::new().fg(Color::Ansi256(208));
        assert_eq!(
            style.paint(ColorChoice::Colored, ColorDepth::Ansi256, "x"),
            "\x1b[38;5;208mx\x1b[0m"
        );
    }

    #[test]
    fn grey_ramp_and_cube_corners_map_to_themselves() {
        assert_eq!(nearest_ansi256((95, 135, 175)), 16 + 36 + 6 * 2 + 3);
        assert_eq!(nearest_ansi256((8, 8, 8)), 232);
        assert_eq!(nearest_ansi256((238, 238, 238)), 255);
    }

    #[rstest]
    #[case::truecolor(Some("truecolor"), Some("xterm"), ColorDepth::TrueColor)]
    #[case::bits(Some("24bit"), None, ColorDepth::TrueColor)]
    #[case::palette(None, Some("xterm-256color"), ColorDepth::Ansi256)]
    #[case::basic(None, Some("xterm"), ColorDepth::Basic)]
    #[case::nothing(None, None, ColorDepth::Basic)]
    fn terminal_announces_its_depth(
        #[case] colorterm: Option<&str>,
        #[case] term: Option<&str>,
        #[case] depth: ColorDepth,
    ) {
        assert_eq!(
            ColorDepth::from_vars(colorterm.map(OsStr::new), term.map(OsStr::new)),
            depth
        );
    }

    #[test]
    fn a_custom_theme_changes_only_its_role() {
        let orange = Style::new().fg(Color::Rgb(255, 120, 0)).bold();
        let custom =
            Skin::new(ColorChoice::Colored).with_theme(Theme::clack().active(orange));
        let clack = Skin::new(ColorChoice::Colored);
        assert_eq!(
            custom.paint(custom.theme().active, "x"),
            "\x1b[38;2;255;120;0m\x1b[1mx\x1b[0m"
        );
        assert_eq!(
            custom.paint(custom.theme().rail, "x"),
            clack.paint(clack.theme().rail, "x")
        );
    }

    #[test]
    fn plain_skin_paints_nothing_with_any_theme() {
        let skin = Skin::new(ColorChoice::Plain).with_theme(Theme::mono());
        assert_eq!(skin.paint(skin.theme().active, "x"), "x");
        assert_eq!(skin.code("run `ls`"), "run `ls`");
    }

    #[test]
    fn code_between_backticks_takes_the_code_style() {
        let skin = Skin::new(ColorChoice::Colored);
        assert_eq!(skin.code("run `ls` now"), "run \x1b[36mls\x1b[0m now");
    }
}
