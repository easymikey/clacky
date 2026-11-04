//! The [`Prompt`] trait, which describes a question with pure functions, and
//! the [`Ask`] trait that [`Session::ask`] runs.

use std::{
    fmt::{self, Display},
    marker::PhantomData,
    ops::ControlFlow,
    str::FromStr,
    time::Duration,
};

use zeroize::Zeroizing;

pub use crate::values::line::Key;
use crate::{
    core::{
        error::{Error, Result},
        keys::{KeyMap, Printable},
        session::{Session, no_input, stopped_by},
    },
    render::{
        color::ColorChoice,
        draw::Echo,
        messages::{Messages, Refusal},
        style::{Skin, Style, Theme},
        symbols::Symbols,
    },
    terminal::{Terminal, abort::Abort},
    values::line::Cancellation,
};

/// What a key did to the state `S` of a prompt.
#[derive(Debug)]
pub enum Next<S> {
    /// The question stays open.
    Editing(S),
    /// The answer is refused; the question stays open under this warning.
    Refuse(S, Refusal),
    /// The answer is final; [`Prompt::finish`] turns the state into it.
    Submit(S),
    /// The person backed out; the state is what the question shows struck
    /// through.
    Quit(S, Cancellation),
}

impl<S> Next<S> {
    /// Applies `f` to the state and keeps the step.
    pub fn map_state<T>(self, f: impl FnOnce(S) -> T) -> Next<T> {
        match self {
            Self::Editing(state) => Next::Editing(f(state)),
            Self::Refuse(state, why) => Next::Refuse(f(state), why),
            Self::Submit(state) => Next::Submit(f(state)),
            Self::Quit(state, quit) => Next::Quit(f(state), quit),
        }
    }
}

/// Which picture of the question [`Prompt::view`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase<'a> {
    /// Open: the rows between the question and the closing corner.
    Active,
    /// Open after an answer was refused for this reason.
    Warned(&'a str),
    /// Answered: the rows that stay under the question.
    Submitted,
    /// Backed out: what the answer stood at, drawn struck through.
    Cancelled,
}

/// What a prompt does when its input is redirected (a pipe or a file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redirected {
    /// Fails with [`Error::NoInput`].
    NoInput,
    /// The next line is the answer, read as [`Prompt::typed`] says.
    Line,
}

/// What a question with exactly one option does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhenSingle {
    /// Asks all the same.
    #[default]
    Ask,
    /// Takes the option without asking; the answered frame still shows.
    Skip,
}

/// The rows a prompt draws, with the session's skin and the terminal width.
#[derive(Debug)]
pub struct Frame {
    skin: Skin,
    width: Option<usize>,
    rows: Vec<String>,
}

impl Frame {
    #[cfg(test)]
    pub(crate) const fn new(color: ColorChoice, width: Option<usize>) -> Self {
        Self::skinned(Skin::new(color), width)
    }

    pub(crate) const fn skinned(skin: Skin, width: Option<usize>) -> Self {
        Self {
            skin,
            width,
            rows: Vec::new(),
        }
    }

    /// Whether the session paints in color.
    #[must_use]
    pub const fn color(&self) -> ColorChoice {
        self.skin.color()
    }

    /// The style of each role; apply one with [`paint`](Self::paint).
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        self.skin.theme()
    }

    /// The glyphs the session draws with.
    #[must_use]
    pub const fn symbols(&self) -> &Symbols {
        self.skin.symbols()
    }

    /// The texts the session shows.
    #[must_use]
    pub const fn messages(&self) -> &Messages {
        self.skin.messages()
    }

    /// `text` in `style`; plain when the output takes no color.
    #[must_use]
    pub fn paint(&self, style: Style, text: &str) -> String {
        self.skin.paint(style, text)
    }

    pub(crate) const fn skin(&self) -> &Skin {
        &self.skin
    }

    /// The width in columns; `None` when unknown.
    #[must_use]
    pub const fn width(&self) -> Option<usize> {
        self.width
    }

    pub(crate) fn reset(&mut self, width: Option<usize>) {
        self.width = width;
        self.rows.clear();
    }

    pub(crate) const fn rows_mut(&mut self) -> &mut Vec<String> {
        &mut self.rows
    }

    /// Adds a row. Leave out cursor-move escapes: the runner owns the cursor.
    pub fn row(&mut self, row: impl Into<String>) {
        self.rows.push(row.into());
    }

    /// The rows added so far.
    #[must_use]
    pub fn rows(&self) -> &[String] {
        &self.rows
    }
}

impl Extend<String> for Frame {
    fn extend<I: IntoIterator<Item = String>>(&mut self, rows: I) {
        self.rows.extend(rows);
    }
}

/// One interactive question as pure functions, with no terminal and no clock.
pub trait Prompt: Sized {
    /// What the question answers with.
    type Output;
    /// What changes while the person answers, such as a cursor or typed text.
    type State;

    /// The question, on the line with the diamond.
    fn message(&self) -> &str;

    /// The state the question opens with.
    ///
    /// # Errors
    /// There is nothing to ask, for example a choice without options.
    fn start(&self) -> Result<Self::State>;

    /// Applies one key to `state`.
    #[must_use]
    fn update(&self, state: Self::State, key: Key) -> Next<Self::State>;

    /// Draws `state` in the given phase.
    fn view(&self, state: &Self::State, phase: Phase<'_>, frame: &mut Frame);

    /// The answer a submitted state stands for.
    ///
    /// # Errors
    /// The state names no answer.
    fn finish(self, state: Self::State) -> Result<Self::Output>;

    /// Rows shown under the question on a terminal that has no keys.
    #[must_use]
    fn listing(&self, _messages: &Messages) -> Vec<String> {
        Vec::new()
    }

    /// The keys a typed line stands for on a terminal without keys. By default
    /// the characters, then Enter.
    #[must_use]
    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        line.trim()
            .chars()
            .map(Key::Char)
            .chain([Key::Enter])
            .collect()
    }

    /// The key a keystroke stands for before the key map reads it. By default
    /// the keystroke itself; a prompt whose answers are words of the session's
    /// `messages` reads their first letters here.
    #[must_use]
    fn spelled(&self, key: Key, _messages: &Messages) -> Key {
        key
    }

    /// Why a typed line is refused before any key is applied to it. By default
    /// none is.
    #[must_use]
    fn misread(&self, _line: &str, _messages: &Messages) -> Option<Refusal> {
        None
    }

    /// How typed characters show; a secret is masked and read with echo off.
    #[must_use]
    fn echo(&self) -> Echo {
        Echo::Shown
    }

    /// How the prompt treats printable keys: as text (the default), or as keys
    /// the session's [`KeyMap`] may bind to actions.
    #[must_use]
    fn printable(&self) -> Printable {
        Printable::Typed
    }

    /// What the prompt does when its input is redirected. By default it fails
    /// with [`Error::NoInput`].
    #[must_use]
    fn redirected(&self) -> Redirected {
        Redirected::NoInput
    }

    /// The state a refused answer leaves behind. By default the same state.
    #[must_use]
    fn refused(&self, state: Self::State) -> Self::State {
        state
    }

    /// The state Enter takes the answer from when the caller set a default,
    /// `None` otherwise. A question without a default cannot be answered under
    /// [`Defaults::Take`](crate::core::settings::Defaults::Take).
    #[must_use]
    fn by_default(&self) -> Option<Self::State> {
        None
    }

    /// With exactly one option, what the question does about it and the state
    /// that has it picked.
    #[must_use]
    fn sole(&self) -> Option<(WhenSingle, Self::State)> {
        None
    }

    /// Takes the answer from `line`, say a variable or a file, without asking.
    #[must_use]
    fn given_line(self, line: Option<&str>) -> GivenLine<Self> {
        GivenLine {
            inner: self,
            line: line.map(|line| Zeroizing::new(line.to_owned())),
        }
    }

    /// Runs the question on `keys` without a terminal.
    ///
    /// # Errors
    /// Esc gives [`Error::Cancelled`], Ctrl-C [`Error::Interrupted`], keys
    /// that run out before the answer [`Error::NoInput`], and keys that run
    /// out after a refused answer [`Error::Refused`].
    fn simulate(self, keys: impl IntoIterator<Item = Key>) -> Result<Self::Output> {
        let state = self.start()?;
        let map = KeyMap::clack();
        let messages = Messages::english();
        let flow =
            keys.into_iter()
                .try_fold((state, None::<Refusal>), |(state, _), key| {
                    match self.update(state, read_key(&self, key, &map, &messages)) {
                        Next::Editing(state) => ControlFlow::Continue((state, None)),
                        Next::Refuse(state, why) => {
                            ControlFlow::Continue((state, Some(why)))
                        }
                        Next::Submit(state) => ControlFlow::Break(Ok(state)),
                        Next::Quit(_, quit) => {
                            ControlFlow::Break(Err(stopped_by(quit)))
                        }
                    }
                });
        match flow {
            ControlFlow::Break(Ok(done)) => self.finish(done),
            ControlFlow::Break(Err(error)) => Err(error),
            ControlFlow::Continue((_, Some(reason))) => Err(Error::Refused {
                question: self.message().to_owned(),
                reason: reason.text(&Messages::english()),
            }),
            ControlFlow::Continue((_, None)) => Err(no_input(&self)),
        }
    }
}

pub(crate) fn read_key<P: Prompt>(
    prompt: &P,
    keystroke: Key,
    keys: &KeyMap,
    messages: &Messages,
) -> Key {
    let spelled = prompt.spelled(keystroke, messages);
    if spelled == keystroke {
        keys.translate(keystroke, prompt.printable())
    } else {
        spelled
    }
}

/// A prompt whose answer can be read before it is final, so that
/// [`validate`](Self::validate) and [`parse`](Self::parse) can check it.
pub trait Peek: Prompt {
    /// What a check sees: a `&str` for text, a `&V` for a choice, a `Vec<&V>`
    /// for several choices.
    type Peeked<'a>
    where
        Self: 'a;

    /// The answer `state` stands for, by reference; `None` while there is none
    /// yet.
    #[must_use]
    fn peek<'a>(&'a self, state: &'a Self::State) -> Option<Self::Peeked<'a>>;

    /// Keeps the question open while `check` returns an error; the error's text
    /// is the warning.
    #[must_use]
    fn validate<E: Display, F>(self, check: F) -> Validate<Self, F>
    where
        F: for<'a> Fn(Self::Peeked<'a>) -> std::result::Result<(), E>,
    {
        Validate { inner: self, check }
    }

    /// Answers with the text read as a `T`; while it does not parse, the
    /// question stays open under the `T::Err` message.
    #[must_use]
    fn parse<T: FromStr>(self) -> Parse<Self, T>
    where
        for<'a> Self::Peeked<'a>: AsRef<str>,
        T::Err: Display,
    {
        Parse {
            inner: self,
            output: PhantomData,
        }
    }
}

/// A prompt that refuses the answers its check rejects; see [`Peek::validate`].
#[derive(Debug)]
pub struct Validate<P, F> {
    inner: P,
    check: F,
}

impl<P: Peek, F, E: Display> Prompt for Validate<P, F>
where
    F: for<'a> Fn(P::Peeked<'a>) -> std::result::Result<(), E>,
{
    type Output = P::Output;
    type State = P::State;

    fn message(&self) -> &str {
        self.inner.message()
    }

    fn start(&self) -> Result<Self::State> {
        self.inner.start()
    }

    fn update(&self, state: Self::State, key: Key) -> Next<Self::State> {
        match self.inner.update(state, key) {
            Next::Submit(state) => {
                let refusal = self
                    .inner
                    .peek(&state)
                    .and_then(|answer| (self.check)(answer).err())
                    .map(|why| Refusal::Said(why.to_string()));
                match refusal {
                    Some(why) => Next::Refuse(self.inner.refused(state), why),
                    None => Next::Submit(state),
                }
            }
            other @ (Next::Editing(_) | Next::Refuse(..) | Next::Quit(..)) => other,
        }
    }

    fn view(&self, state: &Self::State, phase: Phase<'_>, frame: &mut Frame) {
        self.inner.view(state, phase, frame);
    }

    fn finish(self, state: Self::State) -> Result<Self::Output> {
        self.inner.finish(state)
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        self.inner.listing(messages)
    }

    fn typed(&self, line: &str, messages: &Messages) -> Vec<Key> {
        self.inner.typed(line, messages)
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        self.inner.misread(line, messages)
    }

    fn spelled(&self, key: Key, messages: &Messages) -> Key {
        self.inner.spelled(key, messages)
    }

    fn echo(&self) -> Echo {
        self.inner.echo()
    }

    fn printable(&self) -> Printable {
        self.inner.printable()
    }

    fn redirected(&self) -> Redirected {
        self.inner.redirected()
    }

    fn refused(&self, state: Self::State) -> Self::State {
        self.inner.refused(state)
    }

    fn by_default(&self) -> Option<Self::State> {
        self.inner.by_default()
    }

    fn sole(&self) -> Option<(WhenSingle, Self::State)> {
        self.inner.sole()
    }
}

impl<P: Peek, F, E: Display> Peek for Validate<P, F>
where
    F: for<'a> Fn(P::Peeked<'a>) -> std::result::Result<(), E>,
{
    type Peeked<'a>
        = P::Peeked<'a>
    where
        Self: 'a;

    fn peek<'a>(&'a self, state: &'a Self::State) -> Option<P::Peeked<'a>> {
        self.inner.peek(state)
    }
}

/// A prompt whose text answer is parsed as a `T`; see [`Peek::parse`].
pub struct Parse<P, T> {
    inner: P,
    output: PhantomData<fn() -> T>,
}

impl<P: fmt::Debug, T> fmt::Debug for Parse<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parse")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<P: Peek, T: FromStr> Parse<P, T>
where
    for<'a> P::Peeked<'a>: AsRef<str>,
    T::Err: Display,
{
    fn submitted(&self, state: P::State) -> Next<(P::State, Option<T>)> {
        let read = self
            .inner
            .peek(&state)
            .map(|answer| answer.as_ref().parse::<T>());
        match read {
            Some(Ok(value)) => Next::Submit((state, Some(value))),
            Some(Err(why)) => Next::Refuse(
                (self.inner.refused(state), None),
                Refusal::Said(why.to_string()),
            ),
            None => Next::Submit((state, None)),
        }
    }
}

impl<P: Peek, T: FromStr> Prompt for Parse<P, T>
where
    for<'a> P::Peeked<'a>: AsRef<str>,
    T::Err: Display,
{
    type Output = T;
    type State = (P::State, Option<T>);

    fn message(&self) -> &str {
        self.inner.message()
    }

    fn start(&self) -> Result<Self::State> {
        self.inner.start().map(|state| (state, None))
    }

    fn update(&self, (state, _): Self::State, key: Key) -> Next<Self::State> {
        match self.inner.update(state, key) {
            Next::Submit(state) => self.submitted(state),
            other @ (Next::Editing(_) | Next::Refuse(..) | Next::Quit(..)) => {
                other.map_state(|kept| (kept, None))
            }
        }
    }

    fn view(&self, (state, _): &Self::State, phase: Phase<'_>, frame: &mut Frame) {
        self.inner.view(state, phase, frame);
    }

    fn finish(self, (_, value): Self::State) -> Result<T> {
        value.ok_or_else(|| no_input(&self.inner))
    }

    fn listing(&self, messages: &Messages) -> Vec<String> {
        self.inner.listing(messages)
    }

    fn typed(&self, line: &str, messages: &Messages) -> Vec<Key> {
        self.inner.typed(line, messages)
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        self.inner.misread(line, messages)
    }

    fn spelled(&self, key: Key, messages: &Messages) -> Key {
        self.inner.spelled(key, messages)
    }

    fn echo(&self) -> Echo {
        self.inner.echo()
    }

    fn printable(&self) -> Printable {
        self.inner.printable()
    }

    fn redirected(&self) -> Redirected {
        self.inner.redirected()
    }

    fn refused(&self, (state, value): Self::State) -> Self::State {
        (self.inner.refused(state), value)
    }

    fn by_default(&self) -> Option<Self::State> {
        self.inner.by_default().map(|state| (state, None))
    }

    fn sole(&self) -> Option<(WhenSingle, Self::State)> {
        self.inner.sole().map(|(lone, state)| (lone, (state, None)))
    }
}

impl<P: Peek, T: FromStr> Peek for Parse<P, T>
where
    for<'a> P::Peeked<'a>: AsRef<str>,
    T::Err: Display,
{
    type Peeked<'a>
        = &'a T
    where
        Self: 'a;

    fn peek<'a>(&'a self, (_, value): &'a Self::State) -> Option<&'a T> {
        value.as_ref()
    }
}

/// A prompt that can show an answer the caller already has, which is what
/// [`given`](Self::given) prints.
pub trait Prefill: Prompt {
    /// The text the answered frame shows for `answer`; `None` when the question
    /// cannot have that answer.
    #[must_use]
    fn shown_for(&self, answer: &Self::Output, messages: &Messages) -> Option<String>;

    /// Why [`shown_for`](Self::shown_for) found no text for `answer`. By
    /// default the answer is not one of the options.
    #[must_use]
    fn refusal_of(&self, _answer: &Self::Output, messages: &Messages) -> Refusal {
        Refusal::Said(messages.not_an_option.to_string())
    }

    /// Takes `answer` without asking; the value comes back unchecked, and with
    /// `None` the question is asked.
    #[must_use]
    fn given(self, answer: Option<Self::Output>) -> Given<Self> {
        Given {
            inner: self,
            answer,
        }
    }
}

impl<P: Prefill, F, E: Display> Prefill for Validate<P, F>
where
    F: for<'a> Fn(P::Peeked<'a>) -> std::result::Result<(), E>,
    P: Peek,
{
    fn shown_for(&self, answer: &Self::Output, messages: &Messages) -> Option<String> {
        self.inner.shown_for(answer, messages)
    }

    fn refusal_of(&self, answer: &Self::Output, messages: &Messages) -> Refusal {
        self.inner.refusal_of(answer, messages)
    }
}

impl<P: Prefill, T: FromStr + Display> Prefill for Parse<P, T>
where
    P: Peek,
    for<'a> P::Peeked<'a>: AsRef<str>,
    T::Err: Display,
{
    fn shown_for(&self, answer: &T, _messages: &Messages) -> Option<String> {
        Some(answer.to_string())
    }
}

/// A question that may come with its answer; see [`Prefill::given`].
pub struct Given<P: Prompt> {
    inner: P,
    answer: Option<P::Output>,
}

impl<P: Prompt + fmt::Debug> fmt::Debug for Given<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("inner", &self.inner)
            .field("given", &self.answer.is_some())
            .finish()
    }
}

impl<P: Prefill> Ask for Given<P> {
    type Output = P::Output;

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<P::Output> {
        match self.answer {
            Some(answer) => session.take_given(&self.inner, answer),
            None => session.drive(self.inner),
        }
    }
}

/// A question that may come with its answer as a line of text; see
/// [`Prompt::given_line`].
pub struct GivenLine<P> {
    inner: P,
    line: Option<Zeroizing<String>>,
}

impl<P: fmt::Debug> fmt::Debug for GivenLine<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GivenLine")
            .field("inner", &self.inner)
            .field("given", &self.line.is_some())
            .finish()
    }
}

impl<P: Prompt> Ask for GivenLine<P> {
    type Output = P::Output;

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<P::Output> {
        match self.line {
            Some(line) => session.take_line(self.inner, &line),
            None => session.drive(self.inner),
        }
    }
}

/// Anything [`Session::ask`] can run: a [`Prompt`], or a tuple of up to eight
/// of them, asked in order and answered in order.
pub trait Ask: Sized {
    /// What the question gives back once answered.
    type Output;

    /// # Errors
    /// The terminal failed, or the person stopped the question.
    fn run<T: Terminal>(self, session: &Session<T>) -> Result<Self::Output>;

    /// Passes the answer through `f`.
    #[must_use]
    fn map<O, F: FnOnce(Self::Output) -> O>(self, f: F) -> Map<Self, F> {
        Map { inner: self, f }
    }

    /// Ends the question from outside when `abort` fires, with
    /// [`Error::Aborted`]; an `abort` that fired before the question started
    /// ends it at once.
    #[doc(alias = "signal")]
    #[must_use]
    fn abort(self, abort: &Abort) -> Aborting<Self> {
        Aborting {
            inner: self,
            abort: abort.clone(),
            timeout: None,
        }
    }

    /// Ends the question `wait` after it is asked, the way
    /// [`abort`](Self::abort) does.
    #[must_use]
    fn timeout(self, wait: Duration) -> Aborting<Self> {
        Aborting {
            inner: self,
            abort: Abort::default(),
            timeout: Some(wait),
        }
    }
}

impl<P: Prompt> Ask for P {
    type Output = P::Output;

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<P::Output> {
        session.drive(self)
    }
}

/// A question whose answer passes through a function; see [`Ask::map`].
pub struct Map<A, F> {
    inner: A,
    f: F,
}

impl<A: fmt::Debug, F> fmt::Debug for Map<A, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Map")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<A: Ask, O, F: FnOnce(A::Output) -> O> Ask for Map<A, F> {
    type Output = O;

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<O> {
        self.inner.run(session).map(self.f)
    }
}

/// A question that an [`Abort`] or a timeout may end; see [`Ask::abort`] and
/// [`Ask::timeout`]. With several, the first to fire ends the question.
#[derive(Debug)]
pub struct Aborting<A> {
    inner: A,
    abort: Abort,
    timeout: Option<Duration>,
}

impl<A: Ask> Ask for Aborting<A> {
    type Output = A::Output;

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<A::Output> {
        let abort = self
            .timeout
            .map_or_else(Abort::default, Abort::after)
            .both(&self.abort);
        let _under = session.under(&abort);
        self.inner.run(session)
    }
}

macro_rules! ask_tuple {
    ($($kind:ident $var:ident),+) => {
        impl<$($kind: Ask),+> Ask for ($($kind,)+) {
            type Output = ($($kind::Output,)+);

            fn run<T: Terminal>(self, session: &Session<T>) -> Result<Self::Output> {
                let ($($var,)+) = self;
                Ok(($($var.run(session)?,)+))
            }
        }
    };
}

ask_tuple!(A a, B b);
ask_tuple!(A a, B b, C c);
ask_tuple!(A a, B b, C c, D d);
ask_tuple!(A a, B b, C c, D d, E e);
ask_tuple!(A a, B b, C c, D d, E e, F f);
ask_tuple!(A a, B b, C c, D d, E e, F f, G g);
ask_tuple!(A a, B b, C c, D d, E e, F f, G g, H h);

/// Turns Esc into a value instead of an error.
pub trait CancelAsNone<T> {
    /// `Ok(None)` when the person pressed Esc; Ctrl-C stays
    /// [`Error::Interrupted`].
    ///
    /// # Errors
    /// Any error but [`Error::Cancelled`].
    fn cancel_as_none(self) -> Result<Option<T>>;
}

impl<T> CancelAsNone<T> for Result<T> {
    fn cancel_as_none(self) -> Result<Option<T>> {
        match self {
            Ok(answer) => Ok(Some(answer)),
            Err(Error::Cancelled) => Ok(None),
            Err(other) => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use crate::{
        core::{
            ask::{Ask, CancelAsNone, Key, Peek, Prefill, Prompt},
            error::{Error, Result},
            session::Session,
            settings::Settings,
        },
        fake::{Script, screen, testing::bare},
        prompts::{
            autocomplete::autocomplete_multiselect,
            confirm::confirm,
            group_multiselect::group_multiselect,
            multiselect::multiselect,
            select::{Select, select},
            text::text,
        },
        render::color::ColorChoice,
        terminal::TerminalKind,
    };

    fn kinds() -> Select<u8> {
        select("Kind").item(1_u8, "Library").item(2, "Binary")
    }

    #[test]
    fn simulate_reads_the_letters_of_the_labels_as_a_session_does() {
        let asked = confirm("Weiter?").labels("Ja", "Nein").vertical();
        assert_eq!(asked.simulate([Key::Char('j')]).ok(), Some(true));
    }

    #[test]
    fn a_tuple_asks_in_order_and_answers_in_order() -> Result<()> {
        let ui = bare(Script::new(
            TerminalKind::Live,
            [Key::Down, Key::Enter, Key::Enter],
            [],
        ));
        let (kind, color) = ui.ask((
            kinds(),
            select("Color").item("red", "Red").item("blue", "Blue"),
        ))?;
        assert_eq!((kind, color), (2, "red"));
        let shown = screen(&ui.terminal().written());
        assert!(shown.contains("◇  Kind\n│  Binary\n│\n"), "{shown}");
        assert!(shown.contains("◇  Color\n│  Red\n"), "{shown}");
        Ok(())
    }

    #[test]
    fn a_tuple_stops_at_the_first_question_that_fails() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Cancel], []));
        let asked = ui.ask((kinds(), kinds()));
        assert!(matches!(asked, Err(Error::Cancelled)), "{asked:?}");
        let shown = screen(&ui.terminal().written());
        assert_eq!(shown.matches("Kind").count(), 1, "{shown}");
    }

    #[test]
    fn map_changes_the_answer_and_keeps_the_question() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Down, Key::Enter], []));
        let name = ui.ask(kinds().map(|kind| format!("kind {kind}")))?;
        assert_eq!(name, "kind 2");
        let both = bare(Script::new(
            TerminalKind::Live,
            [Key::Enter, Key::Down, Key::Enter],
            [],
        ));
        assert_eq!(both.ask((kinds(), kinds()).map(|(a, b)| a + b))?, 3);
        Ok(())
    }

    #[test]
    fn esc_is_none_and_ctrl_c_stays_an_error() {
        let esc = bare(Script::new(TerminalKind::Live, [Key::Cancel], []));
        assert!(matches!(esc.ask(kinds()).cancel_as_none(), Ok(None)));
        let ctrl_c = bare(Script::new(TerminalKind::Live, [Key::Interrupt], []));
        let asked = ctrl_c.ask(kinds()).cancel_as_none();
        assert!(matches!(asked, Err(Error::Interrupted)), "{asked:?}");
        let enter = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        assert!(matches!(enter.ask(kinds()).cancel_as_none(), Ok(Some(1))));
    }

    #[test]
    fn simulate_ends_with_a_typed_error_when_there_is_no_answer() {
        let esc = kinds().simulate([Key::Down, Key::Cancel]);
        assert!(matches!(esc, Err(Error::Cancelled)), "{esc:?}");
        let ctrl_c = kinds().simulate([Key::Interrupt]);
        assert!(matches!(ctrl_c, Err(Error::Interrupted)), "{ctrl_c:?}");
        let short = kinds().simulate([Key::Down]);
        assert!(
            matches!(short, Err(Error::NoInput { ref question }) if question == "Kind"),
            "{short:?}"
        );
    }

    #[test]
    fn validate_keeps_a_choice_open_under_a_warning() -> Result<()> {
        let ui = bare(Script::new(
            TerminalKind::Live,
            [Key::Enter, Key::Down, Key::Enter],
            [],
        ));
        let prompt = kinds().validate(|kind| {
            if *kind == 1 {
                Err("pick the binary")
            } else {
                Ok(())
            }
        });
        assert_eq!(ui.ask(prompt)?, 2);
        let frames = ui.terminal().frames();
        assert!(
            frames.iter().any(|frame| frame.contains("▲  Kind\n")),
            "{frames:?}"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.contains("└  pick the binary")),
            "{frames:?}"
        );
        let shown = screen(&ui.terminal().written());
        assert_eq!(shown.matches('▲').count(), 0, "{shown}");
        assert!(shown.ends_with("◇  Kind\n│  Binary\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn the_warned_frame_is_yellow() -> Result<()> {
        let colors = Settings::new(ColorChoice::Colored);
        let script = Script::new(TerminalKind::Live, [Key::Enter, Key::Enter], []);
        let ui = Session::new(script, colors);
        let refuses_once = std::cell::Cell::new(true);
        ui.ask(kinds().validate(|_| {
            if refuses_once.replace(false) {
                Err("not yet")
            } else {
                Ok(())
            }
        }))?;
        let raw = ui.terminal().written();
        assert!(raw.contains("\x1b[33m▲\x1b[0m"), "{raw:?}");
        assert!(raw.contains("\x1b[33m└\x1b[0m"), "{raw:?}");
        assert!(raw.contains("\x1b[33mnot yet\x1b[0m"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn a_refusal_on_a_blind_terminal_asks_again() {
        let blind = bare(Script::new(TerminalKind::Dumb, [], ["1"]));
        let asked = blind.ask(kinds().validate(|_| Err::<(), _>("no")));
        assert!(matches!(asked, Err(Error::Cancelled)), "{asked:?}");
        let shown = blind.terminal().written();
        assert!(shown.contains("▲  no\n◆  Kind"), "{shown:?}");
    }

    fn warned(ui: &Session<Script>, warning: &str) -> bool {
        let line = format!("└  {warning}");
        ui.terminal()
            .frames()
            .iter()
            .any(|frame| frame.contains(&line))
    }

    fn raw_after(ui: &Session<Script>) -> String {
        ui.terminal().written()
    }

    #[test]
    fn validate_sees_the_picked_values_of_a_multiselect() -> Result<()> {
        let keys = [
            Key::Char(' '),
            Key::Enter,
            Key::Down,
            Key::Char(' '),
            Key::Enter,
        ];
        let ui = bare(Script::new(TerminalKind::Live, keys, []));
        let prompt = multiselect("Tools")
            .item("fmt", "Formatter")
            .item("lint", "Linter")
            .validate(|picked| {
                if picked.len() < 2 {
                    Err("pick two")
                } else {
                    Ok(())
                }
            });
        assert_eq!(ui.ask(prompt)?, ["fmt", "lint"]);
        assert!(warned(&ui, "pick two"));
        Ok(())
    }

    #[test]
    fn validate_sees_the_picked_values_of_a_group_multiselect() -> Result<()> {
        let keys = [
            Key::Char(' '),
            Key::Enter,
            Key::Down,
            Key::Down,
            Key::Down,
            Key::Char(' '),
            Key::Enter,
        ];
        let ui = bare(Script::new(TerminalKind::Live, keys, []));
        let prompt = group_multiselect("Tools")
            .group("Rust", [("fmt", "Formatter"), ("lint", "Linter")])
            .group("Web", [("bundle", "Bundler")])
            .validate(|picked| {
                if picked.len() < 3 {
                    Err("pick all three")
                } else {
                    Ok(())
                }
            });
        assert_eq!(ui.ask(prompt)?, ["fmt", "lint", "bundle"]);
        assert!(warned(&ui, "pick all three"));
        Ok(())
    }

    #[test]
    fn validate_sees_the_picked_values_of_an_autocomplete_multiselect() -> Result<()> {
        let keys = [Key::Tab, Key::Enter, Key::Down, Key::Tab, Key::Enter];
        let ui = bare(Script::new(TerminalKind::Live, keys, []));
        let prompt = autocomplete_multiselect("Tools")
            .item("fmt", "Formatter")
            .item("lint", "Linter")
            .validate(|picked| {
                if picked.len() < 2 {
                    Err("pick two")
                } else {
                    Ok(())
                }
            });
        assert_eq!(ui.ask(prompt)?, ["fmt", "lint"]);
        assert!(warned(&ui, "pick two"));
        Ok(())
    }

    #[test]
    fn validate_lends_text_as_a_str() {
        let refused = text("Name").validate(|name: &str| {
            if name.len() < 3 {
                Err("too short")
            } else {
                Ok(())
            }
        });
        let keys = [
            Key::Char('a'),
            Key::Enter,
            Key::Char('b'),
            Key::Char('c'),
            Key::Enter,
        ];
        assert_eq!(refused.simulate(keys).ok().as_deref(), Some("abc"));
    }

    #[test]
    fn given_answers_without_asking_and_shows_the_answered_frame() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        assert_eq!(ui.ask(kinds().given(Some(2)))?, 2);
        let shown = screen(&raw_after(&ui));
        assert!(shown.contains("◇  Kind\n│  Binary"), "{shown}");
        assert!(!shown.contains('◆'), "{shown}");
        Ok(())
    }

    #[test]
    fn given_nothing_asks_as_usual() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Down, Key::Enter], []));
        assert_eq!(ui.ask(kinds().given(None))?, 2);
        Ok(())
    }

    #[test]
    fn given_a_value_that_is_no_option_is_invalid() {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        let asked = ui.ask(kinds().given(Some(9)));
        assert!(
            matches!(&asked, Err(Error::Refused { question, .. }) if question == "Kind"),
            "{asked:?}"
        );
    }

    #[test]
    fn given_line_is_read_and_validated_like_a_piped_line() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], []));
        assert_eq!(
            ui.ask(text("Port").parse::<u16>().given_line(Some("8080")))?,
            8080
        );
        assert_eq!(ui.ask(kinds().given_line(Some("binary")))?, 2);
        assert_eq!(ui.ask(kinds().given_line(Some("1")))?, 1);
        let shown = screen(&raw_after(&ui));
        assert!(shown.contains("◇  Port\n│  8080"), "{shown}");
        let refused = ui.ask(text("Port").parse::<u16>().given_line(Some("http")));
        assert!(
            matches!(&refused, Err(Error::Refused { question, .. }) if question == "Port"),
            "{refused:?}"
        );
        let unknown = ui.ask(kinds().given_line(Some("nope")));
        assert!(
            matches!(&unknown, Err(Error::Refused { question, .. }) if question == "Kind"),
            "{unknown:?}"
        );
        let checked = ui.ask(
            text("Name")
                .validate(|name: &str| {
                    if name == "root" {
                        Err("reserved")
                    } else {
                        Ok(())
                    }
                })
                .given_line(Some("root")),
        );
        assert!(
            matches!(&checked, Err(Error::Refused { reason, .. }) if reason == "reserved"),
            "{checked:?}"
        );
        Ok(())
    }

    #[test]
    fn given_line_nothing_asks_as_usual() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        assert_eq!(ui.ask(kinds().given_line(None))?, 1);
        Ok(())
    }

    #[test]
    fn a_lone_option_is_taken_only_when_told_to() -> Result<()> {
        let one = || select("Kind").item(7_u8, "Only");
        let take = bare(Script::new(TerminalKind::Live, [], []));
        assert_eq!(take.ask(one().skip_if_single())?, 7);
        assert!(screen(&raw_after(&take)).contains("◇  Kind\n│  Only"));
        let ask = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        assert_eq!(ask.ask(one())?, 7);
        assert!(raw_after(&ask).contains('◆'));
        let two = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        assert_eq!(two.ask(kinds().skip_if_single())?, 1);
        Ok(())
    }
}
