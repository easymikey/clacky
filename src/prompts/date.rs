//! The `date` prompt: a calendar date typed or stepped segment by segment.
use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, Redirected},
        error::Result,
    },
    prompts::choice::Step,
    render::{
        color::ColorChoice,
        messages::{Messages, Refusal},
        style::Skin,
    },
    values::date::{Date, DateField, DateOrder, Notice},
};

const SEPARATOR: char = '-';

/// A date, typed as digits or stepped with Up and Down.
/// ```no_run
/// use clacky::{
///     prelude::*,
///     prompts::date::date,
///     values::date::{Date, DateOrder},
/// };
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let day = ui.ask(date("Departure?").order(DateOrder::Dmy))?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn date(message: impl Into<String>) -> DatePrompt {
    DatePrompt {
        message: message.into(),
        order: DateOrder::Ymd,
        default: None,
        min: None,
        max: None,
    }
}

/// The question [`date`] builds.
#[derive(Debug, Clone)]
pub struct DatePrompt {
    message: String,
    order: DateOrder,
    default: Option<Date>,
    min: Option<Date>,
    max: Option<Date>,
}

impl DatePrompt {
    /// In which order the parts are typed and shown; year first by default.
    #[must_use]
    pub fn order(self, order: DateOrder) -> Self {
        Self { order, ..self }
    }

    /// The answer when Enter is pressed on nothing.
    #[must_use]
    pub fn default(self, date: Date) -> Self {
        Self {
            default: Some(date),
            ..self
        }
    }

    /// Earlier dates are refused.
    #[must_use]
    pub fn min(self, date: Date) -> Self {
        Self {
            min: Some(date),
            ..self
        }
    }

    /// Later dates are refused.
    #[must_use]
    pub fn max(self, date: Date) -> Self {
        Self {
            max: Some(date),
            ..self
        }
    }

    fn field(&self) -> DateField {
        let mut field = DateField::new(self.order, SEPARATOR);
        if let Some(date) = self.default {
            field = field.default_value(date);
        }
        if let Some(date) = self.min {
            field = field.min(date);
        }
        if let Some(date) = self.max {
            field = field.max(date);
        }
        field
    }
}

/// The segments being typed, and the date once Enter took it.
#[derive(Debug, Clone)]
pub struct DateState {
    field: DateField,
    done: Option<Date>,
}

impl Prompt for DatePrompt {
    type Output = Date;
    type State = DateState;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<DateState> {
        Ok(DateState {
            field: self.field(),
            done: None,
        })
    }

    fn update(&self, state: DateState, key: Key) -> Next<DateState> {
        let field = state.field.clone();
        match state.field.press(key) {
            Step::Submit(date) => Next::Submit(DateState {
                field,
                done: Some(date),
            }),
            Step::Quit(quit) => Next::Quit(DateState { field, done: None }, quit),
            Step::Editing(mut edited) => {
                let refused = if key == Key::Enter {
                    edited.take_notice()
                } else {
                    None
                };
                let open = DateState {
                    field: edited,
                    done: None,
                };
                match refused {
                    Some(why) => Next::Refuse(open, Refusal::Date(why)),
                    None => Next::Editing(open),
                }
            }
        }
    }

    fn view(&self, state: &DateState, phase: Phase<'_>, frame: &mut Frame) {
        let skin = frame.skin();
        let (theme, symbols) = (skin.theme(), skin.symbols());
        match phase {
            Phase::Active => {
                let bar = skin.paint(theme.active, symbols.bar);
                let line = format!("{bar}  {}", state.field.render(skin));
                let warning = state
                    .field
                    .warning(skin)
                    .map(|warning| format!("{bar}  {warning}"));
                frame.row(line);
                frame.extend(warning);
            }
            Phase::Warned(_) => {
                let bar = skin.paint(theme.warning, symbols.bar);
                frame.row(format!("{bar}  {}", state.field.render(skin)));
            }
            Phase::Submitted => {
                frame.row(state.done.map_or_else(String::new, |date| {
                    self.order.format(date, SEPARATOR)
                }));
            }
            Phase::Cancelled => {
                frame.row(state.field.render(&Skin::new(ColorChoice::Plain)));
            }
        }
    }

    fn finish(self, state: DateState) -> Result<Date> {
        state.done.ok_or(crate::core::error::Error::NoInput)
    }

    /// An ISO line `YYYY-MM-DD` typed in this order; an empty line is Enter.
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        let digits = line.trim().parse::<Date>().map_or_else(
            |_| String::new(),
            |date| {
                let mut digits = self.order.format(date, SEPARATOR);
                digits.retain(|c| c != SEPARATOR);
                digits
            },
        );
        digits.chars().map(Key::Char).chain([Key::Enter]).collect()
    }

    fn misread(&self, line: &str, _messages: &Messages) -> Option<Refusal> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        line.parse::<Date>()
            .is_err()
            .then_some(Refusal::Date(Notice::Unreadable))
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn by_default(&self) -> Option<DateState> {
        self.default.map(|_| DateState {
            field: self.field(),
            done: None,
        })
    }
}

impl Peek for DatePrompt {
    type Peeked<'a> = Date;

    fn peek<'a>(&'a self, state: &'a DateState) -> Option<Date> {
        state.done
    }
}

impl Prefill for DatePrompt {
    fn shown_for(&self, answer: &Date, _messages: &Messages) -> Option<String> {
        Some(self.order.format(*answer, SEPARATOR))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        core::{
            ask::{Key, Phase, Prompt},
            error::Error,
            session::Session,
            settings::Settings,
        },
        fake::{Script, testing},
        prompts::date::date,
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
        values::date::{Date, DateOrder},
    };

    #[expect(clippy::expect_used, reason = "a test names real days")]
    fn day(year: u16, month: u8, day: u8) -> Date {
        Date::new(year, month, day).expect("a real day")
    }

    fn chars(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    fn then_enter(text: &str) -> Vec<Key> {
        let mut keys = chars(text);
        keys.push(Key::Enter);
        keys
    }

    #[rstest]
    #[case(DateOrder::Ymd, "20240517")]
    #[case(DateOrder::Mdy, "05172024")]
    #[case(DateOrder::Dmy, "17052024")]
    fn typing_digits_in_each_order(#[case] order: DateOrder, #[case] typed: &str) {
        let got = date("When?").order(order).simulate(then_enter(typed));
        assert_eq!(got.ok(), Some(day(2024, 5, 17)));
    }

    #[test]
    fn up_steps_a_blank_segment_and_tab_moves_on() {
        let keys = [Key::Up, Key::Tab, Key::Up, Key::Tab, Key::Up, Key::Enter];
        let got = date("When?").order(DateOrder::Dmy).simulate(keys);
        assert_eq!(got.ok(), Some(day(2000, 1, 1)));
    }

    #[test]
    fn enter_on_nothing_takes_the_default() {
        let got = date("When?")
            .default(day(2030, 1, 2))
            .simulate([Key::Enter]);
        assert_eq!(got.ok(), Some(day(2030, 1, 2)));
    }

    #[test]
    fn a_date_before_min_is_warned_then_fixed() {
        let prompt = date("When?").min(day(2024, 6, 1));
        let early = prompt.clone().simulate(then_enter("20240517"));
        assert!(matches!(
            early,
            Err(Error::Refused { reason, .. }) if reason == "Date must be on or after 2024-06-01"
        ));
        let mut keys = then_enter("20240517");
        keys.push(Key::Left);
        keys.extend(then_enter("06"));
        assert_eq!(prompt.simulate(keys).ok(), Some(day(2024, 6, 17)));
    }

    #[test]
    fn a_date_after_max_is_refused() {
        let got = date("When?")
            .max(day(2024, 1, 1))
            .simulate(then_enter("20240517"));
        assert!(matches!(
            got,
            Err(Error::Refused { reason, .. }) if reason == "Date must be on or before 2024-01-01"
        ));
    }

    fn piped(order: DateOrder, line: &'static str) -> Result<Date, Error> {
        let script = Script::new(TerminalKind::Redirected, [], [line]);
        Session::new(script, Settings::new(ColorChoice::Plain))
            .ask(date("When?").order(order))
    }

    #[rstest]
    #[case(DateOrder::Ymd)]
    #[case(DateOrder::Mdy)]
    #[case(DateOrder::Dmy)]
    fn a_piped_iso_line_is_a_date_in_any_order(#[case] order: DateOrder) {
        assert_eq!(
            piped(order, "2024-05-17").map_err(|e| e.to_string()),
            Ok(day(2024, 5, 17))
        );
    }

    #[test]
    fn a_piped_line_that_is_no_date_says_what_a_date_looks_like() {
        let refused = piped(DateOrder::Ymd, "next friday");
        assert!(
            matches!(
                &refused,
                Err(Error::Refused { reason, .. })
                    if *reason == "expected an existing date written as YYYY-MM-DD"
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_piped_line_that_is_no_date_follows_the_messages() {
        let script = Script::new(TerminalKind::Redirected, [], ["next friday"]);
        let ui = Session::new(script, Settings::new(ColorChoice::Plain)).messages(
            Messages::english().date_unreadable("erwartet ein Datum als JJJJ-MM-TT"),
        );
        let refused = ui.ask(date("When?"));
        assert!(
            matches!(
                &refused,
                Err(Error::Refused { reason, .. })
                    if *reason == "erwartet ein Datum als JJJJ-MM-TT"
            ),
            "{refused:?}"
        );
    }

    fn rows(prompt: &impl Prompt, keys: Vec<Key>, phase: Phase<'_>) -> Vec<String> {
        testing::rendered(prompt, keys, phase, ColorChoice::Plain, 40)
    }

    #[test]
    fn active_shows_segments_and_a_digit_warning() {
        let prompt = date("When?");
        assert_eq!(rows(&prompt, vec![], Phase::Active), ["│  yyyy-mm-dd"]);
        let typed = rows(
            &prompt,
            chars("2024").into_iter().chain(chars("13")).collect(),
            Phase::Active,
        );
        assert_eq!(
            typed,
            [
                "│  2024-1m-dd".to_owned(),
                "│  There are only 12 months in a year".to_owned()
            ]
        );
    }

    #[test]
    fn warned_and_submitted_and_cancelled_rows() {
        let prompt = date("When?").order(DateOrder::Dmy);
        assert_eq!(
            rows(&prompt, then_enter("17052024"), Phase::Submitted),
            ["17-05-2024"]
        );
        assert_eq!(
            rows(&prompt, chars("17"), Phase::Warned("x")),
            ["│  17-mm-yyyy"]
        );
        assert_eq!(rows(&prompt, chars("17"), Phase::Cancelled), ["17-mm-yyyy"]);
    }
}
