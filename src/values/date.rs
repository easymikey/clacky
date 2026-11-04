//! A calendar date and the pure machine behind a date prompt: no IO, no clock.

use std::{error::Error, fmt, str::FromStr};

use crate::{
    prompts::choice::Step,
    render::{messages::Messages, style::Skin},
    values::line::{Cancellation, Key},
};

const MAX_YEAR: u16 = 9999;
const SEED_YEAR: u16 = 2000;

/// A day in the proleptic Gregorian calendar, years 1 to 9999.
///
/// Fields are private so that every `Date` is a real day; the order of the
/// fields makes the derived ordering chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// `None` unless the day exists, leap days included.
    #[must_use]
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let in_year = (1..=MAX_YEAR).contains(&year);
        let in_month = (1..=12).contains(&month);
        let length = month_length(Some(year), Some(u16::from(month)));
        let in_day = (1..=length).contains(&u16::from(day));
        (in_year && in_month && in_day).then_some(Self { year, month, day })
    }

    /// The year.
    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    /// The month, 1 to 12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// The text was not an existing day written as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseDateError;

impl fmt::Display for ParseDateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&Messages::english().date_unreadable)
    }
}

impl Error for ParseDateError {}

impl FromStr for Date {
    type Err = ParseDateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ParseDateError);
        };
        let number = |part: &str, width: usize| {
            let digits =
                part.len() == width && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse::<u16>().ok()).flatten()
        };
        let year = number(year, 4).ok_or(ParseDateError)?;
        let month = number(month, 2).and_then(|n| u8::try_from(n).ok());
        let day = number(day, 2).and_then(|n| u8::try_from(n).ok());
        month
            .zip(day)
            .and_then(|(month, day)| Self::new(year, month, day))
            .ok_or(ParseDateError)
    }
}

const fn is_leap(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_length(year: Option<u16>, month: Option<u16>) -> u16 {
    match month {
        Some(2) if year.is_none_or(is_leap) => 29,
        Some(2) => 28,
        Some(4 | 6 | 9 | 11) => 30,
        _ => 31,
    }
}

/// In which order the year, month and day are written and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// Year, month, day.
    Ymd,
    /// Month, day, year.
    Mdy,
    /// Day, month, year.
    Dmy,
}

impl DateOrder {
    const fn segments(self) -> [Segment; 3] {
        match self {
            DateOrder::Ymd => [Segment::Year, Segment::Month, Segment::Day],
            DateOrder::Mdy => [Segment::Month, Segment::Day, Segment::Year],
            DateOrder::Dmy => [Segment::Day, Segment::Month, Segment::Year],
        }
    }

    const fn first(self) -> Segment {
        let [first, _, _] = self.segments();
        first
    }

    fn neighbor(self, at: Segment, forward: bool) -> Option<Segment> {
        let order = self.segments();
        let here = order.iter().position(|segment| *segment == at)?;
        let there = if forward {
            here.checked_add(1)
        } else {
            here.checked_sub(1)
        }?;
        order.get(there).copied()
    }

    /// The date written in this order, as a submitted or cancelled row shows
    /// it.
    #[must_use]
    pub fn format(self, date: Date, separator: char) -> String {
        let parts = self.segments().map(|segment| match segment {
            Segment::Year => format!("{:04}", date.year),
            Segment::Month => format!("{:02}", date.month),
            Segment::Day => format!("{:02}", date.day),
        });
        parts.join(&separator.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Segment {
    Year,
    Month,
    Day,
}

impl Segment {
    const fn width(self) -> usize {
        match self {
            Segment::Year => 4,
            Segment::Month | Segment::Day => 2,
        }
    }

    fn placeholder(self, messages: &Messages) -> &str {
        match self {
            Segment::Year => &messages.year,
            Segment::Month => &messages.month,
            Segment::Day => &messages.day,
        }
    }

    fn highest(self, fields: Fields) -> u16 {
        match self {
            Segment::Year => MAX_YEAR,
            Segment::Month => 12,
            Segment::Day => month_length(fields.year, fields.month),
        }
    }

    const fn settles_on(self, first: u16) -> bool {
        match self {
            Segment::Year => false,
            Segment::Month => first > 1,
            Segment::Day => first > 3,
        }
    }

    const fn refusal(self, value: u16, count: usize) -> Option<Notice> {
        let empty = count >= self.width() && value == 0;
        match self {
            Segment::Year if empty => Some(Notice::YearZero),
            Segment::Month if value > 12 || empty => Some(Notice::MonthRange),
            Segment::Day if value > 31 || empty => Some(Notice::DayRange),
            Segment::Year | Segment::Month | Segment::Day => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fields {
    year: Option<u16>,
    month: Option<u16>,
    day: Option<u16>,
}

impl Fields {
    const fn get(self, segment: Segment) -> Option<u16> {
        match segment {
            Segment::Year => self.year,
            Segment::Month => self.month,
            Segment::Day => self.day,
        }
    }

    const fn set(&mut self, segment: Segment, value: Option<u16>) {
        match segment {
            Segment::Year => self.year = value,
            Segment::Month => self.month = value,
            Segment::Day => self.day = value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Idle,
    Partial(usize),
}

/// What the line says under the input instead of moving on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The month is not 1 to 12.
    MonthRange,
    /// The day does not exist in that month.
    DayRange,
    /// The year is zero.
    YearZero,
    /// A part of the date is missing.
    Incomplete,
    /// A line that is no existing date written `YYYY-MM-DD`.
    Unreadable,
    /// The date is before the earliest allowed.
    BeforeMin(Date),
    /// The date is after the latest allowed.
    AfterMax(Date),
}

impl Notice {
    /// The words of `messages` for the notice.
    #[must_use]
    pub fn text(&self, messages: &Messages) -> String {
        match self {
            Notice::MonthRange => messages.month_range.to_string(),
            Notice::DayRange => messages.day_range.to_string(),
            Notice::YearZero => messages.year_zero.to_string(),
            Notice::Incomplete => messages.date_incomplete.to_string(),
            Notice::Unreadable => messages.date_unreadable.to_string(),
            Notice::BeforeMin(min) => (messages.date_before)(&min.to_string()),
            Notice::AfterMax(max) => (messages.date_after)(&max.to_string()),
        }
    }
}

/// A date being typed: three segments, one of them active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateField {
    order: DateOrder,
    separator: char,
    min: Option<Date>,
    max: Option<Date>,
    default: Option<Date>,
    fields: Fields,
    at: Segment,
    entry: Entry,
    notice: Option<Notice>,
}

impl DateField {
    /// Empty segments, the first one of `order` active.
    #[must_use]
    pub const fn new(order: DateOrder, separator: char) -> Self {
        Self {
            order,
            separator,
            min: None,
            max: None,
            default: None,
            fields: Fields {
                year: None,
                month: None,
                day: None,
            },
            at: order.first(),
            entry: Entry::Idle,
            notice: None,
        }
    }

    /// What Enter submits while every segment is empty.
    #[must_use]
    pub const fn default_value(mut self, date: Date) -> Self {
        self.default = Some(date);
        self
    }

    /// Earlier dates are not submitted; the line warns instead.
    #[must_use]
    pub const fn min(mut self, date: Date) -> Self {
        self.min = Some(date);
        self
    }

    /// Later dates are not submitted; the line warns instead.
    #[must_use]
    pub const fn max(mut self, date: Date) -> Self {
        self.max = Some(date);
        self
    }

    /// Applies one key to the input: still editing, or the finished date.
    #[must_use]
    pub fn press(mut self, key: Key) -> Step<Self, Date> {
        self.notice = None;
        match key {
            Key::Interrupt => Step::Quit(Cancellation::Interrupt),
            Key::Cancel | Key::EndOfInput => Step::Quit(Cancellation::Cancel),
            Key::Enter => self.submit(),
            Key::Left => Step::Editing(self.moved(false)),
            Key::Right => Step::Editing(self.moved(true)),
            Key::Tab => Step::Editing(self.tabbed()),
            Key::Up => Step::Editing(self.stepped(true)),
            Key::Down => Step::Editing(self.stepped(false)),
            Key::Backspace => Step::Editing(self.erased()),
            Key::Char(c) => Step::Editing(self.typed(c)),
            Key::Delete
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Step::Editing(self),
        }
    }

    /// The active line: segments joined by the separator, the active one
    /// reversed, empty ones as dim placeholder letters.
    #[must_use]
    pub fn render(&self, skin: &Skin) -> String {
        let separator = skin.paint(skin.theme().hint, &self.separator.to_string());
        self.order
            .segments()
            .map(|segment| self.cell(segment, skin))
            .join(&separator)
    }

    /// The yellow warning that goes under the line, if there is one.
    #[must_use]
    pub fn warning(&self, skin: &Skin) -> Option<String> {
        self.notice.map(|notice| {
            skin.paint(skin.theme().warning, &notice.text(skin.messages()))
        })
    }

    pub(crate) const fn take_notice(&mut self) -> Option<Notice> {
        self.notice.take()
    }

    fn cell(&self, segment: Segment, skin: &Skin) -> String {
        let theme = skin.theme();
        let active = segment == self.at;
        let width = segment.width();
        let text = match (self.fields.get(segment), self.entry) {
            (None, _) => segment.placeholder(skin.messages()).to_owned(),
            (Some(value), Entry::Partial(typed)) if active => {
                let rest = segment.placeholder(skin.messages()).chars().skip(typed);
                format!("{value:0typed$}{}", rest.collect::<String>())
            }
            (Some(value), Entry::Partial(_) | Entry::Idle) => {
                format!("{value:0width$}")
            }
        };
        match (active, self.fields.get(segment)) {
            (true, _) => skin.paint(theme.cursor, &text),
            (false, None) => skin.paint(theme.placeholder, &text),
            (false, Some(_)) => text,
        }
    }

    fn committed(mut self) -> Self {
        self.entry = Entry::Idle;
        if self.fields.get(self.at) == Some(0) {
            self.fields.set(self.at, None);
        }
        self.normalized()
    }

    fn normalized(mut self) -> Self {
        let length = month_length(self.fields.year, self.fields.month);
        self.fields.day = self.fields.day.map(|day| day.min(length));
        self
    }

    fn moved(self, forward: bool) -> Self {
        let mut next = self.committed();
        next.at = next.order.neighbor(next.at, forward).unwrap_or(next.at);
        next
    }

    fn tabbed(self) -> Self {
        let mut next = self.committed();
        next.at = next
            .order
            .neighbor(next.at, true)
            .unwrap_or_else(|| next.order.first());
        next
    }

    fn stepped(self, up: bool) -> Self {
        let mut next = self.committed();
        let segment = next.at;
        let highest = segment.highest(next.fields);
        let value = match (next.fields.get(segment), segment) {
            (None, Segment::Year) => SEED_YEAR,
            (None, Segment::Month | Segment::Day) if up => 1,
            (None, Segment::Month | Segment::Day) => highest,
            (Some(value), Segment::Year | Segment::Month | Segment::Day) => {
                wrapped(value, highest, up)
            }
        };
        next.fields.set(segment, Some(value));
        next.normalized()
    }

    fn typed(self, c: char) -> Self {
        match c.to_digit(10).and_then(|digit| u16::try_from(digit).ok()) {
            Some(digit) => self.digit(digit),
            None if c == self.separator => self.moved(true),
            None => self,
        }
    }

    fn digit(mut self, digit: u16) -> Self {
        let segment = self.at;
        let (count, value) = match self.entry {
            Entry::Idle => (1, digit),
            Entry::Partial(typed) => (
                typed + 1,
                self.fields.get(segment).unwrap_or(0) * 10 + digit,
            ),
        };
        if let Some(notice) = segment.refusal(value, count) {
            self.notice = Some(notice);
            return self;
        }
        self.fields.set(segment, Some(value));
        if count < segment.width() && !(count == 1 && segment.settles_on(value)) {
            self.entry = Entry::Partial(count);
            return self;
        }
        let mut done = self.committed();
        done.at = done.order.neighbor(segment, true).unwrap_or(segment);
        done
    }

    fn erased(mut self) -> Self {
        let segment = self.at;
        match (self.entry, self.fields.get(segment)) {
            (Entry::Partial(typed), Some(value)) if typed > 1 => {
                self.fields.set(segment, Some(value / 10));
                self.entry = Entry::Partial(typed - 1);
            }
            (Entry::Partial(_), _) | (Entry::Idle, Some(_)) => {
                self.fields.set(segment, None);
                self.entry = Entry::Idle;
            }
            (Entry::Idle, None) => {
                self.at = self.order.neighbor(segment, false).unwrap_or(segment);
            }
        }
        self
    }

    fn chosen(&self) -> Option<Date> {
        let fields = self.fields;
        match (fields.year, fields.month, fields.day) {
            (Some(year), Some(month), Some(day)) => {
                Date::new(year, u8::try_from(month).ok()?, u8::try_from(day).ok()?)
            }
            (None, None, None) => self.default,
            _ => None,
        }
    }

    fn outside_bounds(&self, date: Date) -> Option<Notice> {
        let before = self.min.filter(|min| date < *min).map(Notice::BeforeMin);
        before.or_else(|| self.max.filter(|max| date > *max).map(Notice::AfterMax))
    }

    fn submit(self) -> Step<Self, Date> {
        let mut field = self.committed();
        let verdict = field.chosen().map_or(Err(Notice::Incomplete), |date| {
            field.outside_bounds(date).map_or(Ok(date), Err)
        });
        match verdict {
            Ok(date) => Step::Submit(date),
            Err(notice) => {
                field.notice = Some(notice);
                Step::Editing(field)
            }
        }
    }
}

const fn wrapped(value: u16, highest: u16, up: bool) -> u16 {
    if up {
        if value >= highest { 1 } else { value + 1 }
    } else if value <= 1 {
        highest
    } else {
        value - 1
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use proptest::prelude::{
        Just,
        ProptestConfig,
        Strategy,
        prop,
        prop_assert,
        prop_assert_eq,
        prop_oneof,
        proptest,
    };

    use crate::{
        prompts::choice::Step,
        render::{
            color::{ColorChoice, measure_text_width},
            style::Skin,
        },
        values::{
            date::{Date, DateField, DateOrder, Entry, ParseDateError, month_length},
            line::{Cancellation, Key},
        },
    };

    const EPOCH: Date = Date {
        year: 1,
        month: 1,
        day: 1,
    };

    fn day(iso: &str) -> Date {
        iso.parse().unwrap_or(EPOCH)
    }

    fn keys(spec: &str) -> impl Iterator<Item = Key> + '_ {
        spec.chars().map(|c| match c {
            'L' => Key::Left,
            'R' => Key::Right,
            'U' => Key::Up,
            'D' => Key::Down,
            'T' => Key::Tab,
            'B' => Key::Backspace,
            'E' => Key::Enter,
            other => Key::Char(other),
        })
    }

    fn play(field: DateField, spec: &str) -> Step<DateField, Date> {
        keys(spec)
            .try_fold(field, |field, key| match field.press(key) {
                Step::Editing(next) => Ok(next),
                done @ (Step::Submit(_) | Step::Quit(_)) => Err(done),
            })
            .map_or_else(|done| done, Step::Editing)
    }

    fn shown(field: DateField, spec: &str) -> String {
        match play(field, spec) {
            Step::Editing(field) => {
                let line = field.render(&Skin::new(ColorChoice::Plain));
                match field.warning(&Skin::new(ColorChoice::Plain)) {
                    Some(warning) => format!("{line} ! {warning}"),
                    None => line,
                }
            }
            Step::Submit(date) => format!("submit {date}"),
            Step::Quit(Cancellation::Cancel) => "cancel".to_owned(),
            Step::Quit(Cancellation::Interrupt) => "interrupt".to_owned(),
        }
    }

    fn ymd() -> DateField {
        DateField::new(DateOrder::Ymd, '-')
    }

    fn with_date(mut field: DateField, date: Date) -> DateField {
        field.fields = super::Fields {
            year: Some(date.year),
            month: Some(u16::from(date.month)),
            day: Some(u16::from(date.day)),
        };
        field
    }

    fn filled(iso: &str) -> DateField {
        with_date(ymd(), day(iso))
    }

    #[test]
    fn real_days_only() {
        let table = [
            (2024, 2, 29, true),
            (2023, 2, 29, false),
            (1900, 2, 29, false),
            (2000, 2, 29, true),
            (2024, 4, 31, false),
            (2024, 12, 31, true),
            (2024, 0, 1, false),
            (2024, 13, 1, false),
            (2024, 1, 0, false),
            (0, 1, 1, false),
            (10000, 1, 1, false),
            (9999, 12, 31, true),
        ];
        for (year, month, day, real) in table {
            assert_eq!(
                Date::new(year, month, day).is_some(),
                real,
                "{year}-{month}-{day}"
            );
        }
    }

    #[test]
    fn dates_order_by_the_calendar() {
        assert!(day("2023-12-31") < day("2024-01-01"));
        assert!(day("2024-01-31") < day("2024-02-01"));
        assert!(day("2024-02-01") < day("2024-02-02"));
        assert_eq!(day("2024-02-01"), day("2024-02-01"));
    }

    #[test]
    fn iso_text_round_trips() {
        assert_eq!(day("0042-03-09").to_string(), "0042-03-09");
        let date = Date::from_str("2024-02-29");
        assert_eq!(
            date.map(|d| (d.year(), d.month(), d.day())),
            Ok((2024, 2, 29))
        );
    }

    #[test]
    fn other_text_is_no_date() {
        let table = [
            "",
            "2023-02-29",
            "2024-2-29",
            "24-02-29",
            "2024/02/29",
            "2024-02-29x",
            " 2024-02-29",
            "+024-02-29",
            "2024-02-29-01",
            "0000-01-01",
            "2024-13-01",
        ];
        for text in table {
            assert_eq!(Date::from_str(text), Err(ParseDateError), "{text:?}");
        }
    }

    #[test]
    fn each_order_writes_its_own_way() {
        let date = day("2024-05-06");
        let table = [
            (DateOrder::Ymd, '-', "2024-05-06"),
            (DateOrder::Mdy, '/', "05/06/2024"),
            (DateOrder::Dmy, '.', "06.05.2024"),
        ];
        for (order, separator, written) in table {
            assert_eq!(order.format(date, separator), written);
            let field = with_date(DateField::new(order, separator), date);
            assert_eq!(shown(field, ""), written);
        }
    }

    #[test]
    fn typing_fills_segments_and_moves_on() {
        let table = [
            ("", "yyyy-mm-dd"),
            ("2", "2yyy-mm-dd"),
            ("20", "20yy-mm-dd"),
            ("2024", "2024-mm-dd"),
            ("20240", "2024-0m-dd"),
            ("202402", "2024-02-dd"),
            ("20241", "2024-1m-dd"),
            ("2024129", "2024-12-09"),
            ("2024022", "2024-02-2d"),
            ("2024023", "2024-02-3d"),
            ("20240229", "2024-02-29"),
            ("20-", "0020-mm-dd"),
            ("20240R", "2024-mm-dd"),
            ("20241T", "2024-01-dd"),
            ("2024xx", "2024-mm-dd"),
        ];
        for (typed, expected) in table {
            assert_eq!(shown(ymd(), typed), expected, "{typed}");
        }
    }

    #[test]
    fn two_digit_segments_settle_on_a_large_first_digit() {
        let table = [
            (DateOrder::Mdy, "1", "1m/dd/yyyy"),
            (DateOrder::Mdy, "2", "02/dd/yyyy"),
            (DateOrder::Mdy, "0", "0m/dd/yyyy"),
            (DateOrder::Dmy, "3", "3d/mm/yyyy"),
            (DateOrder::Dmy, "4", "04/mm/yyyy"),
            (DateOrder::Dmy, "31", "31/mm/yyyy"),
        ];
        for (order, typed, expected) in table {
            assert_eq!(
                shown(DateField::new(order, '/'), typed),
                expected,
                "{typed}"
            );
        }
    }

    #[test]
    fn digits_out_of_range_warn_and_are_dropped() {
        let months = "There are only 12 months in a year";
        let days = "There are only 31 days in any month";
        let table = [
            ("202413", format!("2024-1m-dd ! {months}")),
            ("202400", format!("2024-0m-dd ! {months}")),
            ("20240232", format!("2024-02-3d ! {days}")),
            ("20240200", format!("2024-02-0d ! {days}")),
            ("0000", "000y-mm-dd ! There is no year 0".to_owned()),
        ];
        for (typed, expected) in table {
            assert_eq!(shown(ymd(), typed), expected, "{typed}");
        }
    }

    #[test]
    fn a_warning_lasts_one_key() {
        assert_eq!(shown(ymd(), "202413R"), "2024-01-dd");
    }

    #[test]
    fn a_typed_day_is_cut_to_its_month() {
        let table = [
            ("20230229E", "submit 2023-02-28"),
            ("20240229E", "submit 2024-02-29"),
            ("20240431E", "submit 2024-04-30"),
            ("20240131LU", "2024-02-29"),
        ];
        for (typed, expected) in table {
            assert_eq!(shown(ymd(), typed), expected, "{typed}");
        }
    }

    #[test]
    fn stepping_wraps_and_cuts_the_day() {
        let table = [
            ("2024-12-15", "RU", "2024-01-15"),
            ("2024-01-15", "RD", "2024-12-15"),
            ("2024-01-31", "RRU", "2024-01-01"),
            ("2024-01-01", "RRD", "2024-01-31"),
            ("2023-02-28", "RRU", "2023-02-01"),
            ("2024-01-31", "RU", "2024-02-29"),
            ("2023-01-31", "RU", "2023-02-28"),
            ("2024-03-31", "RD", "2024-02-29"),
            ("2024-05-31", "RU", "2024-06-30"),
            ("2024-02-29", "U", "2025-02-28"),
            ("2024-02-29", "D", "2023-02-28"),
            ("2024-02-29", "RRU", "2024-02-01"),
            ("9999-05-06", "U", "0001-05-06"),
            ("0001-05-06", "D", "9999-05-06"),
            ("2024-05-06", "TTTU", "2025-05-06"),
            ("2024-05-06", "LU", "2025-05-06"),
            ("2024-05-06", "RRRRU", "2024-05-07"),
        ];
        for (initial, spec, expected) in table {
            assert_eq!(shown(filled(initial), spec), expected, "{initial} {spec}");
        }
    }

    #[test]
    fn stepping_a_blank_segment_starts_it() {
        let table = [
            ("U", "2000-mm-dd"),
            ("D", "2000-mm-dd"),
            ("RU", "yyyy-01-dd"),
            ("RD", "yyyy-12-dd"),
            ("RRU", "yyyy-mm-01"),
            ("RRD", "yyyy-mm-31"),
            ("2024D", "2024-12-dd"),
            ("202402RD", "2024-02-29"),
        ];
        for (spec, expected) in table {
            assert_eq!(shown(ymd(), spec), expected, "{spec}");
        }
    }

    #[test]
    fn backspace_clears_then_steps_back() {
        let table = [
            ("2024-05-06", "B", "yyyy-05-06"),
            ("2024-05-06", "BB", "yyyy-05-06"),
            ("2024-05-06", "RB", "2024-mm-06"),
            ("2024-05-06", "RBBU", "2025-mm-06"),
            ("2024-05-06", "RRB", "2024-05-dd"),
        ];
        for (initial, spec, expected) in table {
            assert_eq!(shown(filled(initial), spec), expected, "{spec}");
        }
        let typed_table = [
            ("20241B", "2024-mm-dd"),
            ("20B", "2yyy-mm-dd"),
            ("20BB", "yyyy-mm-dd"),
            ("202412BU", "2024-01-dd"),
            ("2024120B", "2024-12-dd"),
        ];
        for (typed, expected) in typed_table {
            assert_eq!(shown(ymd(), typed), expected, "{typed}");
        }
    }

    #[test]
    fn bounds_warn_instead_of_submitting() {
        let min = filled("2024-02-01").min(day("2024-03-01"));
        let before = "2024-02-01 ! Date must be on or after 2024-03-01";
        assert_eq!(shown(min.clone(), "E"), before);
        assert_eq!(shown(min.clone(), "ERUE"), "submit 2024-03-01");
        assert_eq!(shown(min, "EE"), before);
        let max = filled("2024-04-01").max(day("2024-03-01"));
        let after = "2024-04-01 ! Date must be on or before 2024-03-01";
        assert_eq!(shown(max.clone(), "E"), after);
        assert_eq!(shown(max, "ERDE"), "submit 2024-03-01");
        let both = filled("2024-03-01")
            .min(day("2024-03-01"))
            .max(day("2024-03-01"));
        assert_eq!(shown(both, "E"), "submit 2024-03-01");
    }

    #[test]
    fn nothing_submits_until_the_date_is_whole() {
        let incomplete = "Please enter a valid date";
        assert_eq!(shown(ymd(), "E"), format!("yyyy-mm-dd ! {incomplete}"));
        assert_eq!(shown(ymd(), "2024E"), format!("2024-mm-dd ! {incomplete}"));
        assert_eq!(
            shown(ymd(), "202402E"),
            format!("2024-02-dd ! {incomplete}")
        );
        assert_eq!(
            shown(filled("2024-05-06"), "RBE"),
            format!("2024-mm-06 ! {incomplete}")
        );
    }

    #[test]
    fn an_empty_field_submits_its_default() {
        let field = ymd().default_value(day("2024-01-02"));
        assert_eq!(shown(field.clone(), "E"), "submit 2024-01-02");
        assert_eq!(
            shown(field.clone(), "2E"),
            "0002-mm-dd ! Please enter a valid date"
        );
        let bounded = field.min(day("2025-01-01"));
        assert_eq!(
            shown(bounded, "E"),
            "yyyy-mm-dd ! Date must be on or after 2025-01-01"
        );
    }

    #[test]
    fn leaving_the_prompt() {
        let quits = [
            (Key::Interrupt, Cancellation::Interrupt),
            (Key::Cancel, Cancellation::Cancel),
            (Key::EndOfInput, Cancellation::Cancel),
        ];
        for (key, expected) in quits {
            assert!(
                matches!(ymd().press(key), Step::Quit(why) if why == expected),
                "{key:?}"
            );
        }
    }

    #[test]
    fn the_active_segment_is_reversed_and_blanks_are_dim() {
        let skin = Skin::new(ColorChoice::Colored);
        let (dim, reversed) = (skin.theme().hint, skin.theme().cursor);
        let dash = skin.paint(dim, "-");
        let field = |spec: &str| match play(ymd(), spec) {
            Step::Editing(field) => field.render(&skin),
            Step::Submit(_) | Step::Quit(_) => String::new(),
        };
        let year = skin.paint(reversed, "20yy");
        let (month, dd) = (skin.paint(dim, "mm"), skin.paint(dim, "dd"));
        assert_eq!(field("20"), format!("{year}{dash}{month}{dash}{dd}"));
        let blank = skin.paint(reversed, "yyyy");
        assert_eq!(field(""), format!("{blank}{dash}{month}{dash}{dd}"));
        assert_eq!(
            field("2024"),
            format!("2024{dash}{}{dash}{dd}", skin.paint(reversed, "mm"))
        );
    }

    #[test]
    fn a_warning_is_yellow() {
        let skin = Skin::new(ColorChoice::Colored);
        let warned = match play(ymd(), "E") {
            Step::Editing(field) => field.warning(&skin),
            Step::Submit(_) | Step::Quit(_) => None,
        };
        assert_eq!(
            warned,
            Some(skin.paint(skin.theme().warning, "Please enter a valid date"))
        );
    }

    mod laws {
        use super::*;

        fn stroke(separator: char) -> impl Strategy<Value = Key> {
            prop_oneof![
                4 => prop::sample::select(vec!['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])
                    .prop_map(Key::Char),
                1 => Just(Key::Char(separator)),
                1 => Just(Key::Char('x')),
                1 => Just(Key::Left),
                1 => Just(Key::Right),
                1 => Just(Key::Up),
                1 => Just(Key::Down),
                1 => Just(Key::Tab),
                1 => Just(Key::Backspace),
                1 => Just(Key::Enter),
                1 => Just(Key::Delete),
            ]
        }

        fn order() -> impl Strategy<Value = DateOrder> {
            prop::sample::select(vec![DateOrder::Ymd, DateOrder::Mdy, DateOrder::Dmy])
        }

        fn real_date() -> impl Strategy<Value = Date> {
            (1u16..=9999, 1u8..=12, 1u8..=31)
                .prop_filter_map("a real day", |(y, m, d)| Date::new(y, m, d))
        }

        fn drive(field: DateField, strokes: &[Key]) -> (Vec<DateField>, Option<Date>) {
            let mut states = vec![field.clone()];
            let mut field = field;
            for key in strokes {
                match field.press(*key) {
                    Step::Editing(next) => {
                        states.push(next.clone());
                        field = next;
                    }
                    Step::Submit(date) => return (states, Some(date)),
                    Step::Quit(_) => return (states, None),
                }
            }
            (states, None)
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, rng_seed: proptest::test_runner::RngSeed::Fixed(0x5eed), ..ProptestConfig::default() })]

            #[test]
            fn every_state_renders_and_every_submit_is_a_real_date(
                order in order(),
                initial in prop::option::of(real_date()),
                min in prop::option::of(real_date()),
                max in prop::option::of(real_date()),
                default in prop::option::of(real_date()),
                strokes in prop::collection::vec(stroke('/'), 0..60),
            ) {
                let mut field = DateField::new(order, '/');
                field = initial.into_iter().fold(field, with_date);
                field = min.into_iter().fold(field, DateField::min);
                field = max.into_iter().fold(field, DateField::max);
                field = default.into_iter().fold(field, DateField::default_value);
                let (states, submitted) = drive(field, &strokes);
                for state in &states {
                    let plain = state.render(&Skin::new(ColorChoice::Plain));
                    prop_assert_eq!(plain.chars().count(), 10, "{}", plain);
                    let colored = state.render(&Skin::new(ColorChoice::Colored));
                    prop_assert_eq!(measure_text_width(&colored), 10);
                    if state.entry == Entry::Idle {
                        let length = month_length(state.fields.year, state.fields.month);
                        prop_assert!(state.fields.day.is_none_or(|day| day <= length));
                    }
                }
                if let Some(date) = submitted {
                    prop_assert_eq!(Date::new(date.year(), date.month(), date.day()), Some(date));
                    prop_assert!(min.is_none_or(|min| date >= min));
                    prop_assert!(max.is_none_or(|max| date <= max));
                    prop_assert_eq!(date.to_string().parse::<Date>(), Ok(date));
                }
            }

            #[test]
            fn iso_text_round_trips_for_every_real_day(date in real_date()) {
                prop_assert_eq!(date.to_string().parse::<Date>(), Ok(date));
            }
        }
    }
}
