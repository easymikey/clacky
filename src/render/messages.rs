//! Every text the library says itself, as a value: English by default,
//! replaceable one field at a time or as a whole.

use std::borrow::Cow;

use crate::{
    core::error::Error,
    prompts::number::NumberNotice,
    render::settings::settings,
    values::{date::Notice, path::PathNotice},
};

settings! {
    /// One field per built-in text; change one with its builder method. A text
    /// setter takes a literal or a `String`; a text that depends on a value takes
    /// a function.
    ///
    /// ```
    /// use clacky::render::messages::Messages;
    /// let messages = Messages::english()
    ///     .cancelled("Abgebrochen")
    ///     .items_selected(|count| format!("{count} gewählt"));
    /// assert_eq!(messages.cancelled, "Abgebrochen");
    /// assert_eq!((messages.items_selected)(3), "3 gewählt");
    /// ```
    ///
    /// A text built at run time goes in as a `String`:
    ///
    /// ```
    /// use clacky::render::messages::Messages;
    /// let name = String::from("Zurück");
    /// let messages = Messages::english().skip(name);
    /// assert_eq!(messages.skip, "Zurück");
    /// ```
    #[derive(Debug, Clone)]
    #[non_exhaustive]
    pub struct Messages {
        /// The label before the typed text of a search.
        search: Cow<'static, str> => Cow<'static, str>,
        /// An empty pick that is refused, for a list of options.
        pick_an_option: Cow<'static, str> => Cow<'static, str>,
        /// An empty pick that is refused, for a search that lists items.
        pick_an_item: Cow<'static, str> => Cow<'static, str>,
        /// The last row of a derived `Option<E>` field, which answers with nothing.
        skip: Cow<'static, str> => Cow<'static, str>,
        /// What stands for an empty pick in the answer.
        none: Cow<'static, str> => Cow<'static, str>,
        /// How many picks, past a few.
        items_selected: fn(usize) -> String,
        /// A search that finds nothing.
        no_matches: Cow<'static, str> => Cow<'static, str>,
        /// How many options match a search.
        matches: fn(usize) -> String,
        /// The default words of a yes or no question.
        yes: Cow<'static, str> => Cow<'static, str>,
        /// The word for the answer no.
        no: Cow<'static, str> => Cow<'static, str>,
        /// What an interrupted spinner says.
        cancelled: Cow<'static, str> => Cow<'static, str>,
        /// The dim row of keys under the options.
        keys_select: Cow<'static, str> => Cow<'static, str>,
        /// The key row of a multiselect.
        keys_multiselect: Cow<'static, str> => Cow<'static, str>,
        /// The key row of a group multiselect.
        keys_group_multiselect: Cow<'static, str> => Cow<'static, str>,
        /// The key row of an autocomplete.
        keys_autocomplete: Cow<'static, str> => Cow<'static, str>,
        /// The key row of an autocomplete multiselect.
        keys_autocomplete_multiselect: Cow<'static, str> => Cow<'static, str>,
        /// The key row of an autocomplete multiselect while the cursor is in
        /// the list.
        keys_autocomplete_multiselect_navigating: Cow<'static, str> => Cow<'static, str>,
        /// The key row of a number prompt.
        keys_number: Cow<'static, str> => Cow<'static, str>,
        /// The key row of a multiline prompt that submits on Enter.
        keys_multiline_enter: Cow<'static, str> => Cow<'static, str>,
        /// The key row of a multiline prompt that submits on Ctrl-D.
        keys_multiline_ctrl_d: Cow<'static, str> => Cow<'static, str>,
        /// The help line under a numbered list on a terminal without keys.
        listing_name: Cow<'static, str> => Cow<'static, str>,
        /// The help line when options are picked by number.
        listing_numbers: Cow<'static, str> => Cow<'static, str>,
        /// The help line when options are picked by key.
        listing_key: Cow<'static, str> => Cow<'static, str>,
        /// The letters of an empty date segment.
        year: Cow<'static, str> => Cow<'static, str>,
        /// The letters of an empty month segment.
        month: Cow<'static, str> => Cow<'static, str>,
        /// The letters of an empty day segment.
        day: Cow<'static, str> => Cow<'static, str>,
        /// What a date prompt refuses, and why.
        month_range: Cow<'static, str> => Cow<'static, str>,
        /// The refusal for a day out of range.
        day_range: Cow<'static, str> => Cow<'static, str>,
        /// The refusal for year zero.
        year_zero: Cow<'static, str> => Cow<'static, str>,
        /// The refusal for a date with a missing part.
        date_incomplete: Cow<'static, str> => Cow<'static, str>,
        /// The refusal of a line that is no existing date written `YYYY-MM-DD`.
        date_unreadable: Cow<'static, str> => Cow<'static, str>,
        /// Given the earliest date, written out.
        date_before: fn(&str) -> String,
        /// Given the latest date, written out.
        date_after: fn(&str) -> String,
        /// What a path prompt refuses, and why.
        path_empty: Cow<'static, str> => Cow<'static, str>,
        /// Given the path as typed.
        path_missing: fn(&str) -> String,
        /// Given the path as typed.
        path_not_directory: fn(&str) -> String,
        /// What a number prompt refuses when nothing is typed and there is no
        /// default.
        number_required: Cow<'static, str> => Cow<'static, str>,
        /// A number out of range; given the smallest and the largest.
        number_between: fn(&str, &str) -> String,
        /// A number below the smallest, which is given.
        number_at_least: fn(&str) -> String,
        /// A number above the largest, which is given.
        number_at_most: fn(&str) -> String,
        /// Why a given answer is refused: it is no option of the question.
        not_an_option: Cow<'static, str> => Cow<'static, str>,
        /// Why a given line is refused: it names no answer.
        not_an_answer: Cow<'static, str> => Cow<'static, str>,
        /// Why a typed line is refused by a yes or no question: it is neither.
        not_yes_or_no: Cow<'static, str> => Cow<'static, str>,
        /// Why a typed line is refused: it names an option that is disabled.
        option_disabled: Cow<'static, str> => Cow<'static, str>,
        /// The error of a Ctrl-C.
        error_interrupted: Cow<'static, str> => Cow<'static, str>,
        /// The error of Esc or Ctrl-D on an empty line.
        error_cancelled: Cow<'static, str> => Cow<'static, str>,
        /// The error of a question ended from outside (a stop or a timeout).
        error_aborted: Cow<'static, str> => Cow<'static, str>,
        /// The error of input that gave no answer; given the question's label.
        error_no_input: fn(&str) -> String,
        /// The error of a choice prompt without options.
        error_no_choices: Cow<'static, str> => Cow<'static, str>,
        /// The error of a question without a default that must answer by
        /// defaults; given its label.
        error_no_default: fn(&str) -> String,
        /// The error of a question asked while a spinner runs.
        error_busy: Cow<'static, str> => Cow<'static, str>,
        /// The error of two choices with one key; given the key.
        error_duplicate_key: fn(char) -> String,
        /// The error of a terminal that failed.
        error_terminal: Cow<'static, str> => Cow<'static, str>,
        /// The error of a terminal that cannot hide a secret.
        error_echo_not_hidden: Cow<'static, str> => Cow<'static, str>,
    }
}

fn items_selected(count: usize) -> String {
    format!("{count} items selected")
}

fn matches(count: usize) -> String {
    let noun = if count == 1 { "match" } else { "matches" };
    format!("{count} {noun}")
}

fn path_missing(path: &str) -> String {
    format!("{path} does not exist.")
}

fn path_not_directory(path: &str) -> String {
    format!("{path} is not a directory.")
}

fn date_before(min: &str) -> String {
    format!("Date must be on or after {min}")
}

fn date_after(max: &str) -> String {
    format!("Date must be on or before {max}")
}

fn number_between(min: &str, max: &str) -> String {
    format!("Enter a number between {min} and {max}")
}

fn number_at_least(min: &str) -> String {
    format!("Enter a number of at least {min}")
}

fn number_at_most(max: &str) -> String {
    format!("Enter a number of at most {max}")
}

fn error_no_input(question: &str) -> String {
    format!("no input: no answer to {question} from the input")
}

fn error_no_default(question: &str) -> String {
    format!("{question}: no default to answer with")
}

fn error_duplicate_key(key: char) -> String {
    format!("two choices use the key {key}")
}

impl Messages {
    /// The texts of clack, in English.
    #[must_use]
    pub const fn english() -> Self {
        Self {
            search: Cow::Borrowed("Search: "),
            pick_an_option: Cow::Borrowed("Please select at least one option."),
            pick_an_item: Cow::Borrowed("Please select at least one item."),
            skip: Cow::Borrowed("Skip"),
            none: Cow::Borrowed("none"),
            items_selected,
            no_matches: Cow::Borrowed("No matches found"),
            matches,
            yes: Cow::Borrowed("Yes"),
            no: Cow::Borrowed("No"),
            cancelled: Cow::Borrowed("Canceled"),
            keys_select: Cow::Borrowed("↑/↓ to navigate • Enter: confirm"),
            keys_multiselect: Cow::Borrowed(
                "↑/↓ to navigate • Space: select • a: all • Enter: confirm",
            ),
            keys_group_multiselect: Cow::Borrowed(
                "↑/↓ to navigate • Space: select • a: all • Enter: confirm",
            ),
            keys_autocomplete: Cow::Borrowed(
                "↑/↓ to select • Enter: confirm • Type: to search",
            ),
            keys_autocomplete_multiselect: Cow::Borrowed(
                "↑/↓ to navigate • Tab: select • Enter: confirm • Type: to search",
            ),
            keys_autocomplete_multiselect_navigating: Cow::Borrowed(
                "↑/↓ to navigate • Space/Tab: select • Enter: confirm • Type: to search",
            ),
            keys_number: Cow::Borrowed("↑/↓ to change"),
            keys_multiline_enter: Cow::Borrowed(
                "Enter: new line • Enter on an empty line: submit",
            ),
            keys_multiline_ctrl_d: Cow::Borrowed("Enter: new line • Ctrl-D: submit"),
            listing_name: Cow::Borrowed("  a number, or part of a name"),
            listing_numbers: Cow::Borrowed(
                "  numbers to pick, e.g. 1 3; Enter keeps the marked",
            ),
            listing_key: Cow::Borrowed("  a key picks; Enter takes the default"),
            year: Cow::Borrowed("yyyy"),
            month: Cow::Borrowed("mm"),
            day: Cow::Borrowed("dd"),
            month_range: Cow::Borrowed("There are only 12 months in a year"),
            day_range: Cow::Borrowed("There are only 31 days in any month"),
            year_zero: Cow::Borrowed("There is no year 0"),
            date_incomplete: Cow::Borrowed("Please enter a valid date"),
            date_unreadable: Cow::Borrowed(
                "expected an existing date written as YYYY-MM-DD",
            ),
            date_before,
            date_after,
            path_empty: Cow::Borrowed("Type a path."),
            path_missing,
            path_not_directory,
            number_required: Cow::Borrowed("Enter a number"),
            number_between,
            number_at_least,
            number_at_most,
            not_an_option: Cow::Borrowed("not one of the options"),
            not_an_answer: Cow::Borrowed("not an answer"),
            not_yes_or_no: Cow::Borrowed("Type y or n."),
            option_disabled: Cow::Borrowed("That option is disabled."),
            error_interrupted: Cow::Borrowed("interrupted"),
            error_cancelled: Cow::Borrowed("cancelled"),
            error_aborted: Cow::Borrowed("aborted"),
            error_no_input,
            error_no_choices: Cow::Borrowed("nothing to choose from"),
            error_no_default,
            error_busy: Cow::Borrowed("a spinner is running, stop it before asking"),
            error_duplicate_key,
            error_terminal: Cow::Borrowed("the terminal stopped responding"),
            error_echo_not_hidden: Cow::Borrowed(
                "could not hide the input on this terminal, so the secret would be visible",
            ),
        }
    }

    /// What `error` says in these texts; a check or the person's own words
    /// stand as they are.
    #[must_use]
    pub fn error(&self, error: &Error) -> String {
        match error {
            Error::Interrupted => self.error_interrupted.to_string(),
            Error::Cancelled => self.error_cancelled.to_string(),
            Error::Aborted => self.error_aborted.to_string(),
            Error::NoInput { question } => (self.error_no_input)(question),
            Error::NoChoices => self.error_no_choices.to_string(),
            Error::NoDefault { question } => (self.error_no_default)(question),
            Error::Busy => self.error_busy.to_string(),
            Error::DuplicateKey(key) => (self.error_duplicate_key)(*key),
            Error::Terminal(_) => self.error_terminal.to_string(),
            Error::EchoNotHidden(_) => self.error_echo_not_hidden.to_string(),
            Error::Refused { question, reason } => format!("{question}: {reason}"),
        }
    }
}

/// Why an answer is not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A reason with its own text, for example what a check said.
    Said(String),
    /// Nothing picked out of a list of options.
    NoOption,
    /// Nothing picked out of a search's items.
    NoItem,
    /// A date the prompt refuses.
    Date(Notice),
    /// A path the prompt refuses.
    Path(PathNotice),
    /// A number the prompt refuses.
    Number(NumberNotice),
}

impl Refusal {
    /// The text that shows under the question.
    #[must_use]
    pub fn text(&self, messages: &Messages) -> String {
        match self {
            Self::Said(text) => text.clone(),
            Self::NoOption => messages.pick_an_option.to_string(),
            Self::NoItem => messages.pick_an_item.to_string(),
            Self::Date(notice) => notice.text(messages),
            Self::Path(notice) => notice.text(messages),
            Self::Number(notice) => notice.text(messages),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, Messages, Refusal};
    use crate::values::{
        date::{Date, Notice},
        path::PathNotice,
    };

    #[test]
    fn errors_follow_the_messages() {
        let german = Messages::english()
            .error_cancelled("abgebrochen")
            .error_aborted("beendet")
            .error_no_input(|question| format!("{question}: keine Eingabe"))
            .error_no_default(|question| format!("{question}: keine Vorgabe"));
        let cancelled = Error::Cancelled;
        let no_default = Error::NoDefault {
            question: "Name".to_owned(),
        };
        assert_eq!(german.error(&cancelled), "abgebrochen");
        assert_eq!(
            german.error(&Error::NoInput {
                question: "Name".to_owned()
            }),
            "Name: keine Eingabe"
        );
        assert_eq!(german.error(&Error::Aborted), "beendet");
        assert_eq!(german.error(&no_default), "Name: keine Vorgabe");
        assert_eq!(Messages::english().error(&cancelled), cancelled.to_string());
        assert_eq!(
            Messages::english().error(&Error::Busy),
            "a spinner is running, stop it before asking"
        );
    }

    #[test]
    fn built_in_refusals_follow_the_messages() {
        let german = Messages::english()
            .pick_an_item("Bitte mindestens ein Element wählen.")
            .date_before(|min| format!("Frühestens {min}"));
        let min = Date::new(2024, 3, 9).map(Notice::BeforeMin);
        assert_eq!(
            Refusal::NoItem.text(&german),
            "Bitte mindestens ein Element wählen."
        );
        assert_eq!(
            Refusal::Date(Notice::DayRange).text(&Messages::english()),
            "There are only 31 days in any month"
        );
        assert_eq!(
            min.map(|notice| Refusal::Date(notice).text(&german)),
            Some("Frühestens 2024-03-09".to_owned())
        );
        assert_eq!(Refusal::Said("nope".to_owned()).text(&german), "nope");
        let missing = Refusal::Path(PathNotice::Missing("a.txt".to_owned()));
        assert_eq!(missing.text(&Messages::english()), "a.txt does not exist.");
        let own = Messages::english().path_missing(|name| format!("{name} fehlt"));
        assert_eq!(missing.text(&own), "a.txt fehlt");
    }
}
