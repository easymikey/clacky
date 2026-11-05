//! The traits behind `#[derive(Choices)]` and `#[derive(Form)]`.
//!
//! These types can be struct fields out of the box:
//!
//! | field type | question | a check takes | Enter on nothing |
//! |---|---|---|---|
//! | `String` | text | `&str` | an empty string |
//! | `u8`..`u128`, `usize`, `i8`..`i128`, `isize`, `f32`, `f64` | number | `&` the number | refused |
//! | `bool` | yes or no | `bool` | Yes |
//! | `Date` | date | `Date` | refused |
//! | `PathBuf` | path | `&Path` | refused |
//! | an enum with `#[derive(Choices)]` | select | `&` the enum | the first variant |
//! | `Vec<E>` of such an enum | multiselect | `Vec<&E>` | refused unless `optional` |
//! | `Option<String>` | text | none yet | `None` |
//! | `Option<number>` | number | none yet | `None` |
//! | `Option<E>` of such an enum | select with a last "Skip" | none yet | the first variant |
//!
//! # Defaults
//!
//! Under [`Defaults::Take`](crate::core::settings::Defaults::Take) a field with
//! no `default` fails with
//! [`Error::NoDefault`](crate::core::error::Error::NoDefault), except an enum,
//! an `Option`, and an `optional` `Vec`.
//!
//! # Field options
//!
//! `#[ask("Label", ..)]` on a field takes, after the label:
//!
//! - `default = value`: what Enter takes (text, numbers, `bool`, `Date`; on a
//!   `String` a string literal);
//! - `placeholder = "text"`: dim text shown until the first key (text and
//!   numbers);
//! - `validate = path::to::check`: a function of what the "a check takes"
//!   column says, returning `Result<(), E>` with `E: Display`, which keeps the
//!   question open under its message (not on an `Option` field yet);
//! - `optional`: a `Vec` field may be left empty (an `Option<T>` field is
//!   always optional);
//! - `parse`: ask for text and read it as the field type;
//! - `skip`: not asked, filled by `Default`.
//!
//! Any other field type fails to compile with "cannot be asked as a form
//! field".
#![cfg_attr(
    all(feature = "derive", feature = "fake"),
    doc = include_str!("askable_example.md")
)]

use std::{convert::Infallible, fmt::Display, path::PathBuf, str::FromStr};

use crate::{
    core::{
        ask::{Ask, Given, Parse, Peek, Prefill},
        error::Result,
        session::Session,
    },
    prompts::{
        choice::{ByLabel, Choice, Same},
        confirm::{Confirm, Reply, confirm},
        date::{DatePrompt, date},
        multiselect::{MultiSelect, multiselect},
        number::{Number, OrNone, number},
        path::{PathPrompt, path},
        select::{Select, select},
        text::{Text, WithPlaceholder, text},
    },
    terminal::Terminal,
    values::date::Date,
};

/// A type one question can answer with.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be asked as a form field",
    note = "a type with `FromStr` and `Display` can be asked with `#[ask(parse)]`; see \
            `clacky::core::askable`"
)]
pub trait Answer: Sized {
    /// The question that answers with `Self`.
    type Prompt: Ask<Output = Self>;

    /// The question, `message` on the line with the diamond.
    #[must_use]
    fn prompt(message: &str) -> Self::Prompt;
}

/// The options of an enum, in the order they are declared.
pub trait Choices: Sized {
    /// The options, in declaration order.
    #[must_use]
    fn choices() -> Vec<Choice<Self>>;

    /// The label of the option for `self`.
    #[must_use]
    fn label(&self) -> &'static str;

    /// A select over [`choices`](Self::choices) that matches a given answer by
    /// [`label`](Self::label).
    #[must_use]
    fn select(message: &str) -> Select<Self, ByLabel<Self>> {
        select(message)
            .items(Self::choices())
            .by_label(Self::label)
            .default_at(0)
    }
}

/// An [`Answer`] type that takes `placeholder` and `default` options.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`placeholder` and `default` do not go on a `{Self}` field",
    note = "only text, numbers, `bool` and `Date` fields take them; see \
            `clacky::core::askable`"
)]
pub trait Fillable: Answer {
    /// What the type shows dim until the first key.
    type Placeholder;
    /// What Enter takes.
    type Default;

    /// The question with its options, if any are given.
    #[must_use]
    fn filled(
        message: &str,
        placeholder: Option<Self::Placeholder>,
        default: Option<Self::Default>,
    ) -> Self::Prompt;
}

/// An answer that may be empty: a blank line is `None`, anything else is read
/// as a `T`.
#[derive(Debug)]
#[doc(hidden)]
pub struct Blank<T>(Option<T>);

impl<T> Blank<T> {
    /// The answer, `None` for a blank line.
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        self.0
    }
}

impl<T: FromStr> FromStr for Blank<T> {
    type Err = T::Err;

    fn from_str(line: &str) -> Result<Self, T::Err> {
        if line.trim().is_empty() {
            Ok(Self(None))
        } else {
            line.parse().map(|value| Self(Some(value)))
        }
    }
}

impl<T: Display> Display for Blank<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.as_ref().map_or(Ok(()), |value| value.fmt(f))
    }
}

/// A question whose [`Blank`] answer comes back as an `Option`.
#[derive(Debug)]
#[doc(hidden)]
pub struct OrBlank<P>(P);

impl<P> OrBlank<P> {
    /// Reads the [`Blank`] answer of `question` as an `Option`.
    #[must_use]
    pub const fn new(question: P) -> Self {
        Self(question)
    }
}

impl<T, P: Ask<Output = Blank<T>>> Ask for OrBlank<P> {
    type Output = Option<T>;

    fn run<Term: Terminal>(self, session: &Session<Term>) -> Result<Option<T>> {
        self.0.run(session).map(Blank::into_option)
    }
}

/// A question that can take its answer ready-made, as a derived form asks each
/// field.
#[doc(hidden)]
pub trait Giving: Ask {
    /// The question that skips itself when it has an answer.
    type Given: Ask<Output = Self::Output>;

    /// Takes `answer` when there is one (printing the answered frame), asks as
    /// usual otherwise.
    #[must_use]
    fn give(self, answer: Option<Self::Output>) -> Self::Given;
}

impl<P: Prefill> Giving for P {
    type Given = Given<P>;

    fn give(self, answer: Option<P::Output>) -> Given<P> {
        self.given(answer)
    }
}

impl<T, P: Prefill<Output = Blank<T>>> Giving for OrBlank<P> {
    type Given = OrBlank<Given<P>>;

    fn give(self, answer: Option<Option<T>>) -> Self::Given {
        OrBlank(self.0.given(answer.map(Blank)))
    }
}

type Blanked<T> = OrBlank<Parse<Typed, Blank<T>>>;

type Typed = Text<WithPlaceholder>;

fn blanked<T: FromStr>(question: Typed) -> Blanked<T>
where
    T::Err: Display,
{
    OrBlank(question.default(String::new()).parse::<Blank<T>>())
}

fn typed(message: &str, placeholder: Option<&str>, default: Option<String>) -> Typed {
    let question = text(message).placeholder(placeholder.unwrap_or_default());
    match default {
        Some(default) => question.default(default),
        None => question,
    }
}

impl Answer for String {
    type Prompt = Typed;

    fn prompt(message: &str) -> Typed {
        typed(message, None, None)
    }
}

impl Fillable for String {
    type Placeholder = &'static str;
    type Default = &'static str;

    fn filled(
        message: &str,
        placeholder: Option<&'static str>,
        default: Option<&'static str>,
    ) -> Typed {
        typed(message, placeholder, default.map(str::to_owned))
    }
}

impl Answer for Option<String> {
    type Prompt = Blanked<String>;

    fn prompt(message: &str) -> Self::Prompt {
        blanked(typed(message, None, None))
    }
}

impl Fillable for Option<String> {
    type Placeholder = &'static str;
    type Default = Infallible;

    fn filled(
        message: &str,
        placeholder: Option<&'static str>,
        default: Option<Infallible>,
    ) -> Self::Prompt {
        default.map_or_else(
            || blanked(typed(message, placeholder, None)),
            |never| match never {},
        )
    }
}

macro_rules! numbers {
    ($($number:ty),+) => {$(
        impl Answer for $number {
            type Prompt = Number<$number>;

            fn prompt(message: &str) -> Number<$number> {
                number(message)
            }
        }

        impl Fillable for $number {
            type Placeholder = &'static str;
            type Default = $number;

            fn filled(
                message: &str,
                placeholder: Option<&'static str>,
                default: Option<$number>,
            ) -> Number<$number> {
                let question = number::<$number>(message)
                    .placeholder(placeholder.unwrap_or_default());
                match default {
                    Some(default) => question.default(default),
                    None => question,
                }
            }
        }

        impl Answer for Option<$number> {
            type Prompt = OrNone<$number>;

            fn prompt(message: &str) -> OrNone<$number> {
                OrNone::new(number(message))
            }
        }

        impl Fillable for Option<$number> {
            type Placeholder = &'static str;
            type Default = Infallible;

            fn filled(
                message: &str,
                placeholder: Option<&'static str>,
                default: Option<Infallible>,
            ) -> OrNone<$number> {
                default.map_or_else(
                    || {
                        OrNone::new(<$number as Fillable>::filled(
                            message,
                            placeholder,
                            None,
                        ))
                    },
                    |never| match never {},
                )
            }
        }
    )+};
}

numbers!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);

impl Answer for PathBuf {
    type Prompt = PathPrompt<'static>;

    fn prompt(message: &str) -> PathPrompt<'static> {
        path(message)
    }
}

impl Answer for bool {
    type Prompt = Confirm;

    fn prompt(message: &str) -> Confirm {
        confirm(message)
    }
}

impl Fillable for bool {
    type Placeholder = Infallible;
    type Default = bool;

    fn filled(
        message: &str,
        placeholder: Option<Infallible>,
        default: Option<bool>,
    ) -> Confirm {
        match (placeholder, default) {
            (Some(never), _) => match never {},
            (None, Some(yes)) => confirm(message).default(Reply::from(yes)),
            (None, None) => confirm(message),
        }
    }
}

impl Answer for Date {
    type Prompt = DatePrompt;

    fn prompt(message: &str) -> DatePrompt {
        date(message)
    }
}

impl Fillable for Date {
    type Placeholder = Infallible;
    type Default = Date;

    fn filled(
        message: &str,
        placeholder: Option<Infallible>,
        default: Option<Date>,
    ) -> DatePrompt {
        match (placeholder, default) {
            (Some(never), _) => match never {},
            (None, Some(day)) => date(message).default(day),
            (None, None) => date(message),
        }
    }
}

/// Zero or more of the variants; at least one unless the field says `optional`.
impl<E: Choices> Answer for Vec<E> {
    type Prompt = MultiSelect<E, ByLabel<E>>;

    fn prompt(message: &str) -> Self::Prompt {
        multiselect(message).items(E::choices()).by_label(E::label)
    }
}

/// The question of an `Option<E>` field: the variants and a last option,
/// labelled [`Messages::skip`](crate::render::messages::Messages::skip), that
/// answers `None`. Under `Defaults::Take` it takes that last option.
#[derive(Debug)]
#[doc(hidden)]
pub struct Skippable<E> {
    message: String,
    #[expect(clippy::option_option, reason = "no answer, or the answer None")]
    given: Option<Option<E>>,
}

/// Two answers of an `Option<E>` are the same when both are `None` or when
/// their variants have one label.
#[derive(Debug)]
#[doc(hidden)]
pub struct SameOption<E>(fn(&E) -> &'static str);

impl<E> Same<Option<E>> for SameOption<E> {
    fn same(&self, a: &Option<E>, b: &Option<E>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => (self.0)(a) == (self.0)(b),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

impl<E: Choices> Ask for Skippable<E> {
    type Output = Option<E>;

    fn run<Term: Terminal>(self, session: &Session<Term>) -> Result<Option<E>> {
        let skip = session.skin().messages().skip.to_string();
        let choices = E::choices();
        let last = choices.len();
        select(self.message)
            .items(choices.into_iter().map(|choice| choice.map(Some)))
            .item(None, skip)
            .same_as(SameOption(E::label))
            .taken_at(last)
            .given(self.given)
            .run(session)
    }
}

impl<E: Choices> Giving for Skippable<E> {
    type Given = Self;

    fn give(self, answer: Option<Option<E>>) -> Self {
        Self {
            given: answer,
            ..self
        }
    }
}

impl<E: Choices> Answer for Option<E> {
    type Prompt = Skippable<E>;

    fn prompt(message: &str) -> Skippable<E> {
        Skippable {
            message: message.to_owned(),
            given: None,
        }
    }
}
