//! The `text` prompt: one line of free text.

use std::marker::PhantomData;

use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, Redirected},
        error::Result,
    },
    prompts::{caret, password::OnError},
    render::{
        draw::{self, Echo},
        messages::Messages,
    },
    values::line::{Cancellation, CharFilter, TypedAnswer},
};

/// A line of text.
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let name = ui.ask(text("Your name?").placeholder("Ada").default("Ada"))?;
/// let port: u16 = ui.ask(text("Port?").initial("8080").parse::<u16>())?;
/// # Ok(())
/// # }
/// ```
///
/// A placeholder shows only while nothing is typed, so a question cannot have
/// both a placeholder and an initial text:
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = text("Name").placeholder("Ada").initial("Grace");
/// ```
#[must_use]
pub fn text(message: impl Into<String>) -> Text {
    Text {
        field: Field::new(message, Echo::Shown),
        start: PhantomData,
    }
}

/// A question with neither placeholder nor initial text yet.
#[derive(Debug, Clone, Copy)]
pub struct Plain;

/// A question with a placeholder; it takes no initial text.
#[derive(Debug, Clone, Copy)]
pub struct WithPlaceholder;

/// A question with an initial text; it takes no placeholder.
#[derive(Debug, Clone, Copy)]
pub struct WithInitial;

/// The question [`text`] builds; `S` tells how it starts: [`Plain`],
/// [`WithPlaceholder`] or [`WithInitial`].
#[derive(Debug)]
pub struct Text<S = Plain> {
    field: Field,
    start: PhantomData<S>,
}

impl Text {
    /// Dim text shown until the first key.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Text<WithPlaceholder> {
        Text {
            field: Field {
                placeholder: text.into(),
                ..self.field
            },
            start: PhantomData,
        }
    }

    /// Text that starts typed and can be edited.
    #[must_use]
    pub fn initial(self, text: impl Into<String>) -> Text<WithInitial> {
        Text {
            field: Field {
                initial: text.into(),
                ..self.field
            },
            start: PhantomData,
        }
    }
}

impl<S> Text<S> {
    /// The answer when Enter is pressed on nothing.
    #[must_use]
    pub fn default(self, text: impl Into<String>) -> Self {
        Self {
            field: Field {
                default: Some(text.into()),
                ..self.field
            },
            ..self
        }
    }

    /// Which characters the answer takes.
    #[must_use]
    pub fn accept(self, filter: CharFilter) -> Self {
        Self {
            field: Field {
                filter,
                ..self.field
            },
            ..self
        }
    }

    /// What a refused answer becomes on the next try; `Keep` by default.
    #[must_use]
    pub fn on_error(self, on_error: OnError) -> Self {
        Self {
            field: Field {
                on_error,
                ..self.field
            },
            ..self
        }
    }
}

impl<S> Prompt for Text<S> {
    type Output = String;
    type State = TypedAnswer;

    fn message(&self) -> &str {
        &self.field.message
    }

    fn start(&self) -> Result<TypedAnswer> {
        Ok(self.field.start())
    }

    fn update(&self, answer: TypedAnswer, key: Key) -> Next<TypedAnswer> {
        self.field.update(answer, key)
    }

    fn view(&self, answer: &TypedAnswer, phase: Phase<'_>, frame: &mut Frame) {
        self.field.view(answer, phase, frame);
    }

    fn finish(self, answer: TypedAnswer) -> Result<String> {
        let mut text = answer.into_text();
        Ok(std::mem::take(&mut *text))
    }

    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        self.field.typed(line)
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn refused(&self, answer: TypedAnswer) -> TypedAnswer {
        self.field.refused(answer)
    }

    fn by_default(&self) -> Option<TypedAnswer> {
        self.field
            .default
            .is_some()
            .then(|| TypedAnswer::new(self.field.filter))
    }
}

impl<S> Peek for Text<S> {
    type Peeked<'a>
        = &'a str
    where
        Self: 'a;

    fn peek<'a>(&'a self, answer: &'a TypedAnswer) -> Option<&'a str> {
        Some(answer.text())
    }
}

impl<S> Prefill for Text<S> {
    fn shown_for(&self, answer: &String, _messages: &Messages) -> Option<String> {
        Some(answer.clone())
    }
}

#[derive(Debug)]
pub(crate) struct Field {
    pub(crate) message: String,
    pub(crate) filter: CharFilter,
    pub(crate) placeholder: String,
    pub(crate) echo: Echo,
    pub(crate) initial: String,
    pub(crate) default: Option<String>,
    pub(crate) on_error: OnError,
}

impl Field {
    pub(crate) fn new(message: impl Into<String>, echo: Echo) -> Self {
        Self {
            message: message.into(),
            filter: CharFilter::Printable,
            placeholder: String::new(),
            echo,
            initial: String::new(),
            default: None,
            on_error: OnError::Keep,
        }
    }

    pub(crate) fn start(&self) -> TypedAnswer {
        TypedAnswer::new(self.filter).prefilled(&self.initial)
    }

    pub(crate) fn update(&self, answer: TypedAnswer, key: Key) -> Next<TypedAnswer> {
        match key {
            Key::Enter => Next::Submit(self.submitted(answer)),
            Key::Interrupt => Next::Quit(answer, Cancellation::Interrupt),
            Key::Cancel => Next::Quit(answer, Cancellation::Cancel),
            Key::EndOfInput if answer.text().is_empty() => {
                Next::Quit(answer, Cancellation::Cancel)
            }
            Key::Char(_)
            | Key::EndOfInput
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::Up
            | Key::Down
            | Key::Tab
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(answer.edit(key)),
        }
    }

    /// Trimmed when shown (a secret keeps its spaces), or the default when
    /// nothing is left.
    fn submitted(&self, answer: TypedAnswer) -> TypedAnswer {
        let answer = match self.echo {
            Echo::Shown => answer.trimmed(),
            Echo::Masked(_) => answer,
        };
        match &self.default {
            Some(default) if answer.text().is_empty() => {
                TypedAnswer::new(self.filter).prefilled(default)
            }
            Some(_) | None => answer,
        }
    }

    pub(crate) fn refused(&self, answer: TypedAnswer) -> TypedAnswer {
        match self.on_error {
            OnError::Keep => answer,
            OnError::Clear => TypedAnswer::new(self.filter),
        }
    }

    pub(crate) fn view(
        &self,
        answer: &TypedAnswer,
        phase: Phase<'_>,
        frame: &mut Frame,
    ) {
        let (theme, symbols) = (frame.theme(), frame.symbols());
        match phase {
            Phase::Active => {
                let bar = frame.paint(theme.active, symbols.bar);
                self.rows(answer, &bar, frame);
            }
            Phase::Warned(_) => {
                let bar = frame.paint(theme.warning, symbols.bar);
                self.rows(answer, &bar, frame);
            }
            Phase::Submitted | Phase::Cancelled => {
                frame.row(draw::masked(self.echo, symbols, answer.text()));
            }
        }
    }

    fn rows(&self, answer: &TypedAnswer, bar: &str, frame: &mut Frame) {
        let (text, at) = match self.echo {
            Echo::Shown => (answer.text().to_owned(), answer.cursor()),
            Echo::Masked(mask) => (
                draw::masked(self.echo, frame.symbols(), answer.text()),
                answer.graphemes_before() * mask.glyph(frame.symbols()).len(),
            ),
        };
        let room = frame.width().map(|width| width.saturating_sub(4).max(1));
        frame.extend(
            caret::rows(frame.skin(), (&text, at), &self.placeholder, room)
                .into_iter()
                .map(|row| format!("{bar}  {row}")),
        );
    }

    /// A shown answer is trimmed when it is submitted; a line replaces the
    /// initial text, an empty one keeps it.
    // shortcut: `Prompt::typed` returns a `Vec<Key>`, so a secret line is
    // copied into plain `Key::Char`s that are freed without wiping; the trait
    // shape allows nothing else.
    pub(crate) fn typed(&self, line: &str) -> Vec<Key> {
        let clear =
            (!self.initial.is_empty() && !line.is_empty()).then_some(Key::ClearBefore);
        clear
            .into_iter()
            .chain(line.chars().map(Key::Char))
            .chain([Key::Enter])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::cell::RefCell;
    #[cfg(feature = "tokio")] use std::num::NonZeroU8;

    use rstest::rstest;

    use crate::{
        core::{
            ask::{Frame, Key, Peek, Phase, Prompt},
            error::{Error, Result},
            session::Session,
        },
        fake::{
            Script,
            screen,
            testing::{self, bare},
        },
        prompts::{password::OnError, text::text},
        render::color::ColorChoice,
        terminal::TerminalKind,
        values::line::CharFilter,
    };

    const NOT_EMAIL: &str = "enter an address like you@example.com";
    #[cfg(feature = "tokio")]
    const WRONG_CODE: &str =
        "that code did not match - check the latest email and type it again";

    fn chars(text: &'static str) -> impl Iterator<Item = Key> {
        text.chars().map(Key::Char)
    }

    fn live(keys: impl IntoIterator<Item = Key>) -> Session<Script> {
        bare(Script::new(TerminalKind::Live, keys, []))
    }

    fn email(text: &str) -> Result<(), &'static str> {
        if text.contains('@') {
            Ok(())
        } else {
            Err(NOT_EMAIL)
        }
    }

    fn rendered<P: Prompt>(
        prompt: &P,
        keys: Vec<Key>,
        color: ColorChoice,
    ) -> Vec<String> {
        testing::rendered(prompt, keys, Phase::Active, color, 40)
    }

    #[test]
    fn placeholder_is_dim_behind_the_caret_and_gone_on_the_first_key() {
        let mail = text("Mail").placeholder("you@example.com");
        let empty = rendered(&mail, vec![], ColorChoice::Colored);
        assert!(
            empty
                .first()
                .is_some_and(|row| row.contains("\x1b[7my\x1b[0m")
                    && row.contains("\x1b[2mo\x1b[0m")),
            "{empty:?}"
        );
        let typed = rendered(&mail, vec![Key::Char('a')], ColorChoice::Plain);
        assert_eq!(typed, ["│  a▏"]);
    }

    #[test]
    fn no_placeholder_unless_the_caller_sets_one() {
        assert_eq!(
            rendered(&text("Mail"), vec![], ColorChoice::Plain),
            ["│  ▏"]
        );
    }

    #[test]
    fn initial_is_typed_and_editable() {
        let name = || text("Name").initial("ab");
        assert_eq!(rendered(&name(), vec![], ColorChoice::Plain), ["│  ab▏"]);
        let edited = name().simulate([Key::Backspace, Key::Char('c'), Key::Enter]);
        assert_eq!(edited.ok().as_deref(), Some("ac"));
    }

    #[rstest]
    #[case::empty_enter(vec![Key::Enter], "fallback")]
    #[case::typed_wins(vec![Key::Char('x'), Key::Enter], "x")]
    fn default_answers_an_empty_enter(
        #[case] keys: Vec<Key>,
        #[case] expected: &str,
    ) -> Result<()> {
        let ui = live(keys);
        assert_eq!(ui.ask(text("Name").default("fallback"))?, expected);
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.ends_with(&format!("◇  Name\n│  {expected}\n│")),
            "{shown}"
        );
        Ok(())
    }

    #[test]
    fn default_is_what_parse_sees() {
        let port = text("Port").default("8080").parse::<u16>();
        assert_eq!(port.simulate([Key::Enter]).ok(), Some(8080));
    }

    #[rstest]
    #[case::no_spaces(CharFilter::NoSpaces, "1234")]
    #[case::printable(CharFilter::Printable, "12 34")]
    fn accept_filters_the_typed_characters(
        #[case] filter: CharFilter,
        #[case] expected: &str,
    ) {
        let keys = chars("12 34").chain([Key::Enter]);
        let code = text("Code").accept(filter).simulate(keys);
        assert_eq!(code.ok().as_deref(), Some(expected));
    }

    #[rstest]
    #[case::ctrl_c(Script::new(TerminalKind::Live, [Key::Interrupt], []), "interrupted")]
    #[case::esc(Script::new(TerminalKind::Live, [Key::Cancel], []), "cancelled")]
    #[case::ctrl_d(Script::new(TerminalKind::Live, [Key::EndOfInput], []), "cancelled")]
    #[case::blind_end(Script::new(TerminalKind::Dumb, [], []), "cancelled")]
    #[case::stdin_ended(Script::new(TerminalKind::Redirected, [], []), "no input: stdin ended")]
    fn a_stopped_question_says_why(#[case] script: Script, #[case] message: &str) {
        let err = bare(script).ask(text("Email")).err();
        let said = err.as_ref().map(ToString::to_string).unwrap_or_default();
        assert!(said.starts_with(message), "{err:?}");
    }

    #[test]
    fn a_cancelled_answer_stays_on_the_screen_struck() {
        let ui = live(chars("ab").chain([Key::Cancel]));
        assert!(matches!(ui.ask(text("Q")), Err(Error::Cancelled)));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("■  Q\n│  ab\n│"), "{shown}");
    }

    #[rstest]
    #[case::blind(TerminalKind::Dumb, 1)]
    #[case::piped(TerminalKind::Redirected, 0)]
    fn a_line_answers_without_keys(
        #[case] kind: TerminalKind,
        #[case] discards: usize,
    ) -> Result<()> {
        let ui = bare(Script::new(kind, [], ["  you@example.com "]));
        assert_eq!(ui.ask(text("Mail"))?, "you@example.com");
        assert_eq!(ui.terminal().discards(), discards);
        assert_eq!(ui.terminal().hidden_reads(), 0);
        Ok(())
    }

    #[test]
    fn a_piped_empty_line_takes_the_default() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Redirected, [], [""]));
        assert_eq!(ui.ask(text("Name").default("Ada"))?, "Ada");
        Ok(())
    }

    #[test]
    fn a_piped_answer_is_filtered_and_a_refused_one_is_invalid() {
        let script = Script::new(TerminalKind::Redirected, [], ["nope"]);
        let err = bare(script).ask(text("Email").validate(email)).err();
        let said = err.as_ref().map(ToString::to_string).unwrap_or_default();
        assert!(said.starts_with("Email: enter an address"), "{err:?}");
        let spaced = Script::new(TerminalKind::Redirected, [], ["1 2"]);
        let code = bare(spaced).ask(text("Code").accept(CharFilter::NoSpaces));
        assert_eq!(code.ok().as_deref(), Some("12"));
    }

    #[test]
    fn a_blind_refusal_asks_again_under_the_warning() -> Result<()> {
        let script = Script::new(TerminalKind::Dumb, [], ["nope", "a@b.c"]);
        let ui = bare(script);
        assert_eq!(ui.ask(text("Email").validate(email))?, "a@b.c");
        let raw = ui.terminal().written();
        assert!(
            raw.contains(&format!("▲  {NOT_EMAIL}\n◆  Email")),
            "{raw:?}"
        );
        Ok(())
    }

    #[test]
    fn a_refused_answer_keeps_the_typed_text_and_the_warning_does_not_pile_up()
    -> Result<()> {
        let keys = chars("bad")
            .chain([Key::Enter])
            .chain(chars("@x"))
            .chain([Key::Enter]);
        let ui = live(keys);
        assert_eq!(ui.ask(text("Email").validate(email))?, "bad@x");
        let frames = ui.terminal().frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame
                    .contains(&format!("▲  Email\n│  bad▏\n└  {NOT_EMAIL}"))),
            "{frames:?}"
        );
        let shown = screen(&ui.terminal().written());
        assert_eq!(shown.matches('▲').count(), 0, "{shown}");
        assert!(shown.ends_with("◇  Email\n│  bad@x\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn parse_refuses_what_is_not_a_number_and_takes_the_next_try() {
        let port = || text("Port").parse::<u16>();
        let fixed = port().simulate(
            chars("x")
                .chain([Key::Enter, Key::Backspace])
                .chain(chars("8080"))
                .chain([Key::Enter]),
        );
        assert_eq!(fixed.ok(), Some(8080));
        let refused = port().simulate(chars("x").chain([Key::Enter]));
        assert!(
            matches!(&refused, Err(Error::Refused { question, reason })
                if question == "Port" && reason.contains("invalid digit")),
            "{refused:?}"
        );
    }

    #[test]
    fn validate_then_parse_and_back_both_see_the_answer() {
        let small = text("N")
            .parse::<u8>()
            .validate(|n| if *n < 10 { Ok(()) } else { Err("under ten") });
        assert_eq!(small.simulate([Key::Char('7'), Key::Enter]).ok(), Some(7));
        let long =
            text("N").validate(|s| if s.len() > 1 { Ok(()) } else { Err("two") });
        let refused = long.parse::<u8>().simulate([Key::Char('7'), Key::Enter]);
        assert!(matches!(refused, Err(Error::Refused { .. })), "{refused:?}");
    }

    #[test]
    fn peek_reads_the_answer_before_it_is_final() {
        let prompt = text("N");
        let state = prompt.start().ok();
        assert_eq!(
            state.as_ref().and_then(|state| prompt.peek(state)),
            Some("")
        );
    }

    #[rstest]
    #[case::keep(OnError::Keep, &["ab", "abc"])]
    #[case::clear(OnError::Clear, &["ab", "c"])]
    fn on_error_keeps_or_clears_the_refused_text(
        #[case] on_error: OnError,
        #[case] seen: &[&str],
    ) {
        let asked = RefCell::new(Vec::<String>::new());
        let prompt = text("Q").on_error(on_error).validate(|answer: &str| {
            asked.borrow_mut().push(answer.to_owned());
            if asked.borrow().len() < 2 {
                Err("again")
            } else {
                Ok(())
            }
        });
        let keys = chars("ab")
            .chain([Key::Enter])
            .chain(chars("c"))
            .chain([Key::Enter]);
        assert!(prompt.simulate(keys).is_ok());
        assert_eq!(*asked.borrow(), seen);
    }

    #[test]
    fn a_long_answer_wraps_under_the_rail() {
        let long = "a".repeat(100);
        let mut frame = Frame::new(ColorChoice::Plain, Some(40));
        let prompt = text("Q").initial(long);
        let state = prompt.start();
        if let Ok(state) = state {
            prompt.view(&state, Phase::Active, &mut frame);
        }
        assert!(frame.rows().len() > 2, "{:?}", frame.rows());
        assert!(frame.rows().iter().all(|row| row.starts_with("│  ")));
    }

    #[test]
    fn unread_input_is_dropped_after_an_answer() -> Result<()> {
        let ui = live(chars("12").chain([Key::Enter]));
        assert_eq!(ui.ask(text("Code"))?, "12");
        assert_eq!(ui.terminal().discards(), 1);
        Ok(())
    }

    #[cfg(feature = "tokio")]
    #[rstest]
    #[case::second_try_takes(Script::new(TerminalKind::Live, chars("000000").chain([Key::Enter]).chain(chars("123456")).chain([Key::Enter]), []), 3, Ok(()), 1)]
    #[case::tries_run_out(Script::new(TerminalKind::Live, chars("000000").chain([Key::Enter]).chain(chars("111111")).chain([Key::Enter]), []), 2, Err(WRONG_CODE), 1)]
    #[case::piped_is_asked_once(Script::new(TerminalKind::Redirected, [], ["000000", "123456"]), 3, Err(WRONG_CODE), 0)]
    #[tokio::test]
    async fn until_asks_again_only_while_it_can(
        #[case] script: Script,
        #[case] tries: u8,
        #[case] expected: Result<(), &'static str>,
        #[case] warnings: usize,
    ) -> Result<()> {
        let ui = bare(script);
        let tries = NonZeroU8::new(tries).unwrap_or(NonZeroU8::MIN);
        let done = ui
            .ask_until(
                || text("Code").accept(CharFilter::NoSpaces),
                "Checking",
                async |code: &String| {
                    if code == "123456" {
                        Ok(())
                    } else {
                        Err(WRONG_CODE)
                    }
                },
                |_| Some(WRONG_CODE.to_owned()),
                tries,
            )
            .await?;
        let raw = ui.terminal().written();
        assert_eq!(done, expected);
        assert_eq!(
            raw.matches(&format!("└  {WRONG_CODE}")).count(),
            warnings,
            "{raw:?}"
        );
        Ok(())
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn until_waits_under_the_label_it_is_given() -> Result<()> {
        let ui = live(chars("1").chain([Key::Enter]));
        let done = ui
            .ask_until(
                || text("Code"),
                "Checking the code",
                async |_: &String| Ok::<_, &str>(()),
                |_| None,
                NonZeroU8::MIN,
            )
            .await?;
        assert_eq!(done, Ok(()));
        assert!(ui.terminal().written().contains("Checking the code"));
        Ok(())
    }
}
