//! The `number` prompt: a number typed in, or stepped with the arrow keys.

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, Redirected},
        error::Result,
    },
    prompts::caret,
    render::{
        draw,
        messages::{Messages, Refusal},
    },
    values::line::{Cancellation, CharFilter, TypedAnswer},
};

/// A number, typed in or stepped with Up and Down by the step, stopping at the
/// bounds; only digits, a leading `-` (signed) and one `.` (floats) are typed,
/// Enter on nothing takes the default, and a number out of range is refused.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let port: u16 = ui.ask(number("Port").min(1).max(65535).default(8080))?;
/// let ratio = ui.ask(number::<f64>("Ratio").min(0.0).max(1.0).step(0.1))?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn number<T: Numeric>(message: impl Into<String>) -> Number<T> {
    Number {
        message: message.into(),
        min: None,
        max: None,
        step: T::ONE,
        default: None,
        placeholder: String::new(),
    }
}

/// A number type [`number`] can ask: the integers and the floats.
pub trait Numeric: sealed::Sealed {}

mod sealed {
    use std::{
        fmt::{Debug, Display},
        str::FromStr,
    };

    /// What kind of number a type is, which decides the keys it takes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        Unsigned,
        Signed,
        Float,
    }

    /// The arithmetic of [`Numeric`](super::Numeric) types, kept to the crate.
    pub trait Sealed: Copy + PartialOrd + Debug + Display + FromStr {
        const LOWEST: Self;
        const HIGHEST: Self;
        const ZERO: Self;
        const ONE: Self;
        const KIND: Kind;

        /// The sum, stopping at the type's bounds.
        fn up(self, by: Self) -> Self;
        /// The difference, stopping at the type's bounds.
        fn down(self, by: Self) -> Self;
        /// Whether it is a number (a float can overflow to infinity).
        fn finite(self) -> bool;

        /// The number `text` writes; `None` when it is no number or does not
        /// fit the type.
        fn read(text: &str) -> Option<Self> {
            text.chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '.')
                .then(|| text.parse::<Self>().ok())
                .flatten()
                .filter(|value| value.finite())
        }
    }
}

use sealed::Kind;

macro_rules! integers {
    ($kind:ident: $($number:ty),+) => {$(
        impl sealed::Sealed for $number {
            const LOWEST: Self = <$number>::MIN;
            const HIGHEST: Self = <$number>::MAX;
            const ZERO: Self = 0;
            const ONE: Self = 1;
            const KIND: Kind = Kind::$kind;

            fn up(self, by: Self) -> Self {
                self.saturating_add(by)
            }

            fn down(self, by: Self) -> Self {
                self.saturating_sub(by)
            }

            fn finite(self) -> bool {
                true
            }
        }

        impl Numeric for $number {}
    )+};
}

/// Rounds to the longer decimals of the two, so 0.1 three times is 0.3, not
/// 0.30000000000000004.
fn rounded<T>(from: T, by: T, sum: T) -> T
where
    T: std::fmt::Display + std::str::FromStr + Copy,
{
    let decimals = |number: T| {
        number
            .to_string()
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len())
    };
    let places = decimals(from).max(decimals(by));
    format!("{sum:.places$}").parse().unwrap_or(sum)
}

macro_rules! floats {
    ($($number:ty),+) => {$(
        impl sealed::Sealed for $number {
            const LOWEST: Self = <$number>::MIN;
            const HIGHEST: Self = <$number>::MAX;
            const ZERO: Self = 0.0;
            const ONE: Self = 1.0;
            const KIND: Kind = Kind::Float;

            fn up(self, by: Self) -> Self {
                rounded(self, by, self + by).min(Self::HIGHEST)
            }

            fn down(self, by: Self) -> Self {
                rounded(self, by, self - by).max(Self::LOWEST)
            }

            fn finite(self) -> bool {
                self.is_finite()
            }
        }

        impl Numeric for $number {}
    )+};
}

integers!(Unsigned: u8, u16, u32, u64, u128, usize);
integers!(Signed: i8, i16, i32, i64, i128, isize);
floats!(f32, f64);

/// Why a number is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumberNotice {
    /// Nothing was typed and there is no default.
    Required,
    /// The number is out of range; given the smallest and the largest.
    Between(String, String),
    /// The number is below the smallest, which is given.
    AtLeast(String),
    /// The number is above the largest, which is given.
    AtMost(String),
}

impl NumberNotice {
    /// The words of `messages` for the notice.
    #[must_use]
    pub fn text(&self, messages: &Messages) -> String {
        match self {
            Self::Required => messages.number_required.to_string(),
            Self::Between(min, max) => (messages.number_between)(min, max),
            Self::AtLeast(min) => (messages.number_at_least)(min),
            Self::AtMost(max) => (messages.number_at_most)(max),
        }
    }
}

/// The question [`number`] builds.
#[derive(Debug)]
pub struct Number<T: Numeric> {
    message: String,
    min: Option<T>,
    max: Option<T>,
    step: T,
    default: Option<T>,
    placeholder: String,
}

impl<T: Numeric> Number<T> {
    /// The smallest answer.
    #[must_use]
    pub fn min(self, min: T) -> Self {
        Self {
            min: Some(min),
            ..self
        }
    }

    /// The largest answer.
    #[must_use]
    pub fn max(self, max: T) -> Self {
        Self {
            max: Some(max),
            ..self
        }
    }

    /// What Up and Down add and subtract; 1 by default.
    #[must_use]
    pub fn step(self, step: T) -> Self {
        Self { step, ..self }
    }

    /// The answer when Enter is pressed on nothing.
    #[must_use]
    pub fn default(self, default: T) -> Self {
        Self {
            default: Some(default),
            ..self
        }
    }

    /// Dim text shown until the first key.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Self {
        Self {
            placeholder: text.into(),
            ..self
        }
    }

    fn within(&self, value: T) -> bool {
        self.min.is_none_or(|min| value >= min)
            && self.max.is_none_or(|max| value <= max)
    }

    fn clamped(&self, value: T) -> T {
        let value = self
            .max
            .map_or(value, |max| if value > max { max } else { value });
        self.min
            .map_or(value, |min| if value < min { min } else { value })
    }

    fn opening(&self) -> T {
        self.clamped(self.default.or(self.min).unwrap_or(T::ZERO))
    }

    fn notice(&self) -> NumberNotice {
        let (low, high) = match T::KIND {
            Kind::Unsigned | Kind::Signed => (Some(T::LOWEST), Some(T::HIGHEST)),
            Kind::Float => (None, None),
        };
        match (self.min.or(low), self.max.or(high)) {
            (Some(min), Some(max)) => {
                NumberNotice::Between(min.to_string(), max.to_string())
            }
            (Some(min), None) => NumberNotice::AtLeast(min.to_string()),
            (None, Some(max)) => NumberNotice::AtMost(max.to_string()),
            (None, None) => NumberNotice::Required,
        }
    }

    fn takes(c: char, answer: &TypedAnswer) -> bool {
        let (text, at) = (answer.text(), answer.cursor());
        let before_sign = at == 0 && text.starts_with('-');
        match c {
            '0'..='9' => !before_sign,
            '-' => T::KIND != Kind::Unsigned && at == 0 && !before_sign,
            '.' => T::KIND == Kind::Float && !text.contains('.') && !before_sign,
            _ => false,
        }
    }

    fn stepped(&self, answer: &TypedAnswer, by: fn(T, T) -> T) -> NumberState<T> {
        let value = T::read(answer.text().trim()).map_or_else(
            || self.opening(),
            |value| self.clamped(by(value, self.step)),
        );
        NumberState::holding(value)
    }

    fn submitted(&self, state: NumberState<T>) -> Next<NumberState<T>> {
        let text = state.answer.text().trim();
        let read = if text.is_empty() {
            self.default
                .ok_or(NumberNotice::Required)
                .and_then(|default| self.checked(default))
        } else {
            T::read(text)
                .ok_or_else(|| self.notice())
                .and_then(|value| self.checked(value))
        };
        match read {
            Ok(value) => Next::Submit(NumberState::holding(value)),
            Err(notice) => Next::Refuse(state, Refusal::Number(notice)),
        }
    }

    fn checked(&self, value: T) -> std::result::Result<T, NumberNotice> {
        if self.within(value) {
            Ok(value)
        } else {
            Err(self.notice())
        }
    }
}

/// What the person has typed, and the number once it is submitted.
#[derive(Debug)]
pub struct NumberState<T> {
    answer: TypedAnswer,
    value: Option<T>,
}

impl<T: Numeric> NumberState<T> {
    fn empty() -> Self {
        Self {
            answer: TypedAnswer::new(CharFilter::Printable),
            value: None,
        }
    }

    fn holding(value: T) -> Self {
        Self {
            answer: TypedAnswer::new(CharFilter::Printable)
                .prefilled(&value.to_string()),
            value: Some(value),
        }
    }

    fn edited(self, key: Key) -> Self {
        Self {
            answer: self.answer.edit(key),
            value: None,
        }
    }
}

impl<T: Numeric> Prompt for Number<T> {
    type Output = T;
    type State = NumberState<T>;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<NumberState<T>> {
        Ok(NumberState::empty())
    }

    fn update(&self, state: NumberState<T>, key: Key) -> Next<NumberState<T>> {
        match key {
            Key::Enter => self.submitted(state),
            Key::Interrupt => Next::Quit(state, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(state, Cancellation::Cancel),
            Key::EndOfInput if state.answer.text().is_empty() => {
                Next::Quit(state, Cancellation::Cancel)
            }
            Key::Up => Next::Editing(self.stepped(&state.answer, T::up)),
            Key::Down => Next::Editing(self.stepped(&state.answer, T::down)),
            Key::Char(c) if !Self::takes(c, &state.answer) => Next::Editing(state),
            Key::Char(_)
            | Key::EndOfInput
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::Tab
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(state.edited(key)),
        }
    }

    fn view(&self, state: &NumberState<T>, phase: Phase<'_>, frame: &mut Frame) {
        let (theme, symbols) = (frame.theme(), frame.symbols());
        let answer = &state.answer;
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let style = match phase {
                    Phase::Warned(_) => theme.warning,
                    Phase::Active | Phase::Submitted | Phase::Cancelled => theme.active,
                };
                let bar = frame.paint(style, symbols.bar);
                let room = frame.width().map(|width| width.saturating_sub(4).max(1));
                let rows = caret::rows(
                    frame.skin(),
                    (answer.text(), answer.cursor()),
                    &self.placeholder,
                    room,
                );
                let keys = draw::instructions(
                    frame.skin(),
                    &frame.messages().keys_number,
                    frame.width(),
                );
                frame.extend(
                    rows.into_iter()
                        .map(|row| format!("{bar}  {row}"))
                        .chain(keys),
                );
            }
            Phase::Submitted | Phase::Cancelled => frame.row(answer.text()),
        }
    }

    fn finish(self, state: NumberState<T>) -> Result<T> {
        state
            .value
            .ok_or_else(|| crate::core::session::no_input(&self))
    }

    /// A line that is no number is refused whole, not typed key by key.
    fn misread(&self, line: &str, _messages: &Messages) -> Option<Refusal> {
        let line = line.trim();
        (!line.is_empty() && T::read(line).is_none())
            .then(|| Refusal::Number(self.notice()))
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn by_default(&self) -> Option<NumberState<T>> {
        self.default.map(|_| NumberState::empty())
    }
}

impl<T: Numeric> Peek for Number<T> {
    type Peeked<'a>
        = &'a T
    where
        Self: 'a;

    fn peek<'a>(&'a self, state: &'a NumberState<T>) -> Option<&'a T> {
        state.value.as_ref()
    }
}

impl<T: Numeric> Prefill for Number<T> {
    fn shown_for(&self, answer: &T, _messages: &Messages) -> Option<String> {
        (self.within(*answer) && answer.finite()).then(|| answer.to_string())
    }

    fn refusal_of(&self, _answer: &T, _messages: &Messages) -> Refusal {
        Refusal::Number(self.notice())
    }
}

/// A [`Number`] whose empty answer is `None`.
#[derive(Debug)]
#[doc(hidden)]
pub struct OrNone<T: Numeric>(Number<T>);

impl<T: Numeric> OrNone<T> {
    /// The question that takes Enter on nothing as `None`.
    #[must_use]
    pub const fn new(question: Number<T>) -> Self {
        Self(question)
    }
}

impl<T: Numeric> Prompt for OrNone<T> {
    type Output = Option<T>;
    type State = NumberState<T>;

    fn message(&self) -> &str {
        self.0.message()
    }

    fn start(&self) -> Result<NumberState<T>> {
        self.0.start()
    }

    fn update(&self, state: NumberState<T>, key: Key) -> Next<NumberState<T>> {
        if key == Key::Enter && state.answer.text().trim().is_empty() {
            Next::Submit(state)
        } else {
            self.0.update(state, key)
        }
    }

    fn view(&self, state: &NumberState<T>, phase: Phase<'_>, frame: &mut Frame) {
        self.0.view(state, phase, frame);
    }

    fn finish(self, state: NumberState<T>) -> Result<Option<T>> {
        if state.answer.text().trim().is_empty() {
            Ok(None)
        } else {
            self.0.finish(state).map(Some)
        }
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        self.0.misread(line, messages)
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn by_default(&self) -> Option<NumberState<T>> {
        Some(NumberState::empty())
    }
}

impl<T: Numeric> Prefill for OrNone<T> {
    fn shown_for(&self, answer: &Option<T>, messages: &Messages) -> Option<String> {
        answer
            .as_ref()
            .map_or_else(|| Some(String::new()), |it| self.0.shown_for(it, messages))
    }

    fn refusal_of(&self, _answer: &Option<T>, _messages: &Messages) -> Refusal {
        Refusal::Number(self.0.notice())
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use super::{Number, Numeric, number};
    use crate::{
        core::{
            ask::{Frame, Key, Peek, Phase, Prefill, Prompt},
            error::{Error, Result},
            session::Session,
        },
        fake::{
            Script,
            screen,
            testing::{self, bare},
        },
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    fn chars(text: &'static str) -> impl Iterator<Item = Key> {
        text.chars().map(Key::Char)
    }

    fn live(keys: impl IntoIterator<Item = Key>) -> Session<Script> {
        bare(Script::new(TerminalKind::Live, keys, []))
    }

    fn piped(line: &'static str) -> Session<Script> {
        bare(Script::new(TerminalKind::Redirected, [], [line]))
    }

    fn reason<T>(result: Result<T>) -> String {
        match result {
            Err(Error::Refused { reason, .. }) => reason,
            Ok(_) | Err(_) => String::new(),
        }
    }

    fn rows<T: Numeric>(
        prompt: &Number<T>,
        keys: Vec<Key>,
        phase: Phase<'_>,
    ) -> Vec<String> {
        testing::rendered(prompt, keys, phase, ColorChoice::Plain, 40)
    }

    #[test]
    fn the_type_comes_from_the_bounds_the_binding_or_the_turbofish() {
        let port: u16 = number("Port")
            .min(1)
            .max(65535)
            .default(8080)
            .simulate([Key::Enter])
            .unwrap_or_default();
        assert_eq!(port, 8080);
        let wide =
            number::<u64>("N").simulate(chars("99999999999").chain([Key::Enter]));
        assert_eq!(wide.ok(), Some(99_999_999_999));
        let fallback = number("N").simulate([Key::Char('7'), Key::Enter]);
        assert_eq!(fallback.ok().map(i32::abs), Some(7));
    }

    #[rstest]
    #[case::letters_are_dropped("1a2b3", 123)]
    #[case::unsigned_has_no_sign("-5", 5)]
    #[case::integers_have_no_point("1.5", 15)]
    #[case::spaces_are_dropped(" 4 2 ", 42)]
    fn an_unsigned_integer_takes_only_digits(
        #[case] typed: &'static str,
        #[case] want: u32,
    ) {
        let got = number::<u32>("N").simulate(chars(typed).chain([Key::Enter]));
        assert_eq!(got.ok(), Some(want));
    }

    #[rstest]
    #[case::sign_first("-12", -12)]
    #[case::sign_only_at_the_start("1-2", 12)]
    #[case::second_sign_dropped("--3", -3)]
    fn a_signed_integer_takes_a_leading_minus(
        #[case] typed: &'static str,
        #[case] want: i32,
    ) {
        let got = number::<i32>("N").simulate(chars(typed).chain([Key::Enter]));
        assert_eq!(got.ok(), Some(want));
    }

    #[test]
    fn the_sign_goes_in_front_but_nothing_goes_in_front_of_it() {
        let doubled =
            chars("5").chain([Key::Home, Key::Char('-'), Key::Char('-'), Key::Enter]);
        assert_eq!(number::<i8>("N").simulate(doubled).ok(), Some(-5));
        let in_front = chars("-5").chain([Key::Home, Key::Char('7'), Key::Enter]);
        assert_eq!(number::<i8>("N").simulate(in_front).ok(), Some(-5));
    }

    #[test]
    fn a_float_takes_one_point() {
        let got = number::<f64>("N").simulate(chars("-1.2.5").chain([Key::Enter]));
        assert_eq!(got.ok().map(|n| n.to_string()), Some("-1.25".to_owned()));
        let half = number::<f32>("N").simulate(chars(".5").chain([Key::Enter]));
        assert_eq!(half.ok().map(|n| n.to_string()), Some("0.5".to_owned()));
    }

    #[rstest]
    #[case::from_the_default(vec![Key::Up], 5)]
    #[case::step_up(vec![Key::Up, Key::Up, Key::Up], 7)]
    #[case::step_down(vec![Key::Down, Key::Down], 4)]
    #[case::stops_at_max(vec![Key::Up; 20], 10)]
    #[case::stops_at_min(vec![Key::Down; 20], 1)]
    #[case::typed_then_stepped(vec![Key::Char('8'), Key::Up], 9)]
    #[case::out_of_range_comes_back(vec![Key::Char('9'), Key::Char('9'), Key::Up], 10)]
    fn arrows_step_within_the_bounds(#[case] mut keys: Vec<Key>, #[case] want: u8) {
        keys.push(Key::Enter);
        let got = number::<u8>("N").min(1).max(10).default(5).simulate(keys);
        assert_eq!(got.ok(), Some(want));
    }

    #[test]
    fn an_empty_field_starts_at_the_default_then_the_smallest_then_zero() {
        let start = |prompt: Number<i16>| prompt.simulate([Key::Down, Key::Enter]).ok();
        assert_eq!(start(number("N").default(40).min(10)), Some(40));
        assert_eq!(start(number("N").min(10)), Some(10));
        assert_eq!(start(number("N")), Some(0));
        assert_eq!(start(number("N").min(3).max(9).default(100)), Some(9));
    }

    #[test]
    fn the_step_is_the_callers_and_saturates_at_the_type() {
        let big = number::<u8>("N").step(100);
        let got = big.simulate(chars("200").chain([Key::Up, Key::Enter]));
        assert_eq!(got.ok(), Some(255));
        let low = number::<i8>("N").step(100);
        let down =
            low.simulate(chars("-100").chain([Key::Down, Key::Down, Key::Enter]));
        assert_eq!(down.ok(), Some(i8::MIN));
        let none = number::<u8>("N")
            .simulate(chars("3").chain([Key::Down; 5]).chain([Key::Enter]));
        assert_eq!(none.ok(), Some(0));
        let float = number::<f32>("N").step(f32::MAX);
        let top = float.simulate(chars("9").chain([Key::Up, Key::Up, Key::Enter]));
        assert_eq!(top.ok(), Some(f32::MAX));
    }

    #[test]
    fn a_float_steps_by_its_step() {
        let got = number::<f64>("N").step(0.5).max(1.0).simulate([
            Key::Up,
            Key::Up,
            Key::Up,
            Key::Up,
            Key::Enter,
        ]);
        assert_eq!(got.ok().map(|n| n.to_string()), Some("1".to_owned()));
    }

    #[test]
    fn a_float_step_leaves_no_rounding_noise() {
        let four = [Key::Up; 4].into_iter().chain([Key::Enter]);
        let double = number::<f64>("Ratio")
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .simulate(four);
        assert_eq!(double.ok(), Some(0.3));
        let nine = [Key::Up; 9].into_iter().chain([Key::Enter]);
        let single = number::<f32>("Ratio")
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .simulate(nine);
        assert_eq!(single.ok(), Some(0.8));
    }

    #[test]
    fn a_given_float_must_be_finite() {
        for odd in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let got = live([]).ask(number::<f64>("N").given(Some(odd)));
            assert!(matches!(got, Err(Error::Refused { .. })), "{got:?}");
        }
    }

    #[rstest]
    #[case::default_wins_on_nothing(vec![Key::Enter], Some(8080))]
    #[case::typed_wins(vec![Key::Char('9'), Key::Enter], Some(9))]
    fn an_empty_enter_takes_the_default(
        #[case] keys: Vec<Key>,
        #[case] want: Option<u16>,
    ) {
        let ui = live(keys);
        assert_eq!(ui.ask(number::<u16>("Port").default(8080)).ok(), want);
        let shown = screen(&ui.terminal().written());
        let answer = want.unwrap_or_default();
        assert!(
            shown.ends_with(&format!("◇  Port\n│  {answer}\n│")),
            "{shown}"
        );
    }

    #[test]
    fn an_empty_enter_without_a_default_is_refused() {
        let got = number::<u16>("Port").simulate([Key::Enter]);
        assert_eq!(reason(got), "Enter a number");
    }

    #[rstest]
    #[case::both(number::<u16>("N").min(5).max(9), "11", "Enter a number between 5 and 9")]
    #[case::below(number::<u16>("N").min(5).max(9), "4", "Enter a number between 5 and 9")]
    #[case::int_min_only(number::<u16>("N").min(5), "4", "Enter a number between 5 and 65535")]
    #[case::overflow(number::<u8>("N"), "300", "Enter a number between 0 and 255")]
    #[case::signed_overflow(number::<i8>("N"), "-129", "Enter a number between -128 and 127")]
    #[case::bad_default(number::<u8>("N").max(5).default(9), "", "Enter a number between 0 and 5")]
    fn a_number_out_of_range_is_refused(
        #[case] prompt: Number<impl Numeric>,
        #[case] typed: &'static str,
        #[case] want: &str,
    ) {
        let got = prompt.simulate(chars(typed).chain([Key::Enter]));
        assert_eq!(reason(got), want);
    }

    #[test]
    fn a_float_names_only_the_bounds_the_caller_set() {
        let said = |prompt: Number<f64>, typed: &'static str| {
            reason(prompt.simulate(chars(typed).chain([Key::Enter])))
        };
        assert_eq!(
            said(number("N").min(0.5), "0.1"),
            "Enter a number of at least 0.5"
        );
        assert_eq!(
            said(number("N").max(0.5), "1"),
            "Enter a number of at most 0.5"
        );
        assert_eq!(
            said(number("N").max(0.5), "-"),
            "Enter a number of at most 0.5"
        );
        assert_eq!(
            said(number("N").min(0.0).max(1.0), "2"),
            "Enter a number between 0 and 1"
        );
        let huge = "9".repeat(400);
        let got = number::<f64>("N")
            .simulate(huge.chars().map(Key::Char).chain([Key::Enter]));
        assert_eq!(reason(got), "Enter a number");
        assert_eq!(said(number("N"), "-"), "Enter a number");
    }

    #[test]
    fn a_refused_number_keeps_its_text_under_the_warning() -> Result<()> {
        let keys =
            chars("11").chain([Key::Enter, Key::Backspace, Key::Char('0'), Key::Enter]);
        let ui = live(keys);
        assert_eq!(ui.ask(number::<u8>("N").max(10))?, 10);
        let frames = ui.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains(
                "▲  N\n│  11▏\n│  ↑/↓ to change\n└  Enter a number between 0 and 10"
            )),
            "{frames:?}"
        );
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("◇  N\n│  10\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn the_warned_rail_is_the_warning_colour() {
        let prompt = number::<u8>("N");
        let warned = rows(&prompt, vec![], Phase::Warned("no"));
        let active = rows(&prompt, vec![], Phase::Active);
        assert_eq!(warned, active);
        let mut frame = Frame::new(ColorChoice::Colored, Some(40));
        if let Ok(state) = prompt.start() {
            prompt.view(&state, Phase::Warned("no"), &mut frame);
            let mut calm = Frame::new(ColorChoice::Colored, Some(40));
            prompt.view(&state, Phase::Active, &mut calm);
            assert_ne!(frame.rows(), calm.rows());
        }
    }

    #[test]
    fn the_rows_are_the_field_and_the_hint() {
        let prompt = number::<u16>("Port").placeholder("8080");
        assert_eq!(
            rows(&prompt, vec![], Phase::Active),
            ["│  ▏8080", "│  ↑/↓ to change"]
        );
        assert_eq!(
            rows(&prompt, vec![Key::Char('4'), Key::Char('2')], Phase::Active),
            ["│  42▏", "│  ↑/↓ to change"]
        );
        assert_eq!(
            rows(&prompt, vec![Key::Char('4'), Key::Enter], Phase::Submitted),
            ["4"]
        );
        assert_eq!(
            rows(&prompt, vec![Key::Up], Phase::Active),
            ["│  0▏", "│  ↑/↓ to change"]
        );
    }

    #[rstest]
    #[case::ctrl_c(Key::Interrupt, "interrupted")]
    #[case::esc(Key::Cancel, "cancelled")]
    #[case::ctrl_d(Key::EndOfInput, "cancelled")]
    fn a_stopped_question_says_why(#[case] key: Key, #[case] message: &str) {
        let said = live([key])
            .ask(number::<u8>("N"))
            .err()
            .map(|e| e.to_string());
        assert_eq!(said.as_deref(), Some(message));
    }

    #[test]
    fn a_given_number_is_shown_and_an_out_of_range_one_is_refused() -> Result<()> {
        let ui = live([]);
        assert_eq!(ui.ask(number::<u8>("N").max(10).given(Some(7)))?, 7);
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("◇  N\n│  7\n│"), "{shown}");
        let refused = live([]).ask(number::<u8>("N").max(10).given(Some(11)));
        assert_eq!(reason(refused), "Enter a number between 0 and 10");
        let asked =
            live([Key::Char('3'), Key::Enter]).ask(number::<u8>("N").given(None));
        assert_eq!(asked.ok(), Some(3));
        Ok(())
    }

    #[test]
    fn peek_and_validate_see_the_number() {
        let prompt = number::<u8>("N");
        assert_eq!(
            prompt.start().ok().as_ref().and_then(|s| prompt.peek(s)),
            None
        );
        let even = number::<u8>("N")
            .validate(|n: &u8| if n % 2 == 0 { Ok(()) } else { Err("even") });
        let got = even.simulate(
            chars("3")
                .chain([Key::Enter, Key::Backspace])
                .chain(chars("4"))
                .chain([Key::Enter]),
        );
        assert_eq!(got.ok(), Some(4));
    }

    #[test]
    fn a_shown_for_answer_is_the_number_in_range() {
        let prompt = number::<u8>("N").min(2);
        let messages = crate::render::messages::Messages::english();
        assert_eq!(prompt.shown_for(&5, &messages).as_deref(), Some("5"));
        assert_eq!(prompt.shown_for(&1, &messages), None);
    }

    #[rstest]
    #[case::plain("42", Some(42), "")]
    #[case::spaces("  42  ", Some(42), "")]
    #[case::letters("4x", None, "Enter a number between 0 and 255")]
    #[case::plus("+4", None, "Enter a number between 0 and 255")]
    #[case::overflow("256", None, "Enter a number between 0 and 255")]
    #[case::negative("-1", None, "Enter a number between 0 and 255")]
    #[case::default("", Some(9), "")]
    fn a_typed_line_goes_through_the_same_checks(
        #[case] line: &'static str,
        #[case] want: Option<u8>,
        #[case] said: &str,
    ) {
        let got = piped(line).ask(number::<u8>("N").default(9));
        assert_eq!(got.as_ref().ok().copied(), want);
        assert_eq!(reason(got), said);
    }

    #[test]
    fn a_typed_line_out_of_range_and_a_blind_retry() -> Result<()> {
        let got = piped("11").ask(number::<u8>("N").max(10));
        assert_eq!(reason(got), "Enter a number between 0 and 10");
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["x", "11", "7"]));
        assert_eq!(ui.ask(number::<u8>("N").max(10))?, 7);
        let raw = ui.terminal().written();
        assert!(
            raw.contains("▲  Enter a number between 0 and 10\n◆  N"),
            "{raw:?}"
        );
        let line = Some("12");
        assert_eq!(live([]).ask(number::<u8>("N").given_line(line))?, 12);
        Ok(())
    }
}
