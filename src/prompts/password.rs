//! The `password` prompt: a line that shows only as a mask.

use unicode_segmentation::UnicodeSegmentation as _;
use zeroize::Zeroizing;

#[cfg(feature = "secrecy")] use crate::core::ask::Ask;
use crate::{
    core::{
        ask::{Frame, Key, Next, Peek, Phase, Prefill, Prompt, Redirected},
        error::Result,
    },
    prompts::text::Field,
    render::{
        draw::{Echo, Mask},
        messages::Messages,
    },
    values::line::{CharFilter, TypedAnswer},
};

/// What happens to the typed text when an answer is refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnError {
    /// The text stays typed, to be edited.
    #[default]
    Keep,
    /// The line starts empty again.
    Clear,
}

/// A secret, never trimmed; the answer is wiped from memory when dropped.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let secret = ui.ask(password("Password?").mask('*'))?;
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn password(message: impl Into<String>) -> Password {
    Password {
        field: Field::new(message, Echo::Masked(Mask::Themed)),
    }
}

/// The question [`password`] builds.
#[derive(Debug)]
pub struct Password {
    field: Field,
}

impl Password {
    /// Dim text shown until the first key.
    #[must_use]
    pub fn placeholder(self, text: impl Into<String>) -> Self {
        Self {
            field: Field {
                placeholder: text.into(),
                ..self.field
            },
        }
    }

    /// The character each typed character shows as; the `mask` of the session's
    /// symbols by default.
    #[must_use]
    pub fn mask(self, mask: char) -> Self {
        Self {
            field: Field {
                echo: Echo::Masked(Mask::Char(mask)),
                ..self.field
            },
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
        }
    }

    /// What a refused secret becomes on the next try; `Keep` by default.
    #[must_use]
    pub fn on_error(self, on_error: OnError) -> Self {
        Self {
            field: Field {
                on_error,
                ..self.field
            },
        }
    }

    /// Answers a [`secrecy::SecretString`] instead of a `Zeroizing<String>`.
    #[cfg(feature = "secrecy")]
    #[must_use]
    pub fn secret(self) -> impl Ask<Output = secrecy::SecretString> {
        // Copied to an exact-size box: moving the 256-byte buffer would shrink it by
        // realloc and leave the old block unwiped; `text` is wiped on drop.
        self.map(|text| secrecy::SecretString::from(text.as_str()))
    }
}

impl Prompt for Password {
    type Output = Zeroizing<String>;
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

    fn finish(self, answer: TypedAnswer) -> Result<Zeroizing<String>> {
        Ok(answer.into_text())
    }

    fn typed(&self, line: &str, _messages: &Messages) -> Vec<Key> {
        self.field.typed(line)
    }

    fn echo(&self) -> Echo {
        self.field.echo
    }

    fn redirected(&self) -> Redirected {
        Redirected::Line
    }

    fn refused(&self, answer: TypedAnswer) -> TypedAnswer {
        self.field.refused(answer)
    }
}

impl Peek for Password {
    type Peeked<'a>
        = &'a str
    where
        Self: 'a;

    fn peek<'a>(&'a self, answer: &'a TypedAnswer) -> Option<&'a str> {
        Some(answer.text())
    }
}

impl Prefill for Password {
    fn shown_for(
        &self,
        answer: &Zeroizing<String>,
        _messages: &Messages,
    ) -> Option<String> {
        // Only the length shows, so the secret itself is never copied.
        Some("*".repeat(answer.graphemes(true).count()))
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::{cell::RefCell, num::NonZeroU8};

    use rstest::rstest;
    use zeroize::Zeroizing;

    use super::{OnError, password};
    use crate::{
        core::{
            ask::{Key, Peek, Prefill, Prompt},
            error::{Error, Result},
            session::Session,
        },
        fake::{Script, screen, testing::bare, without_escapes},
        render::{messages::Messages, symbols::Symbols},
        terminal::TerminalKind,
    };

    const WRONG: &str = "that password did not match - try again";

    fn chars(text: &str) -> impl Iterator<Item = Key> {
        text.chars().map(Key::Char)
    }

    fn live(keys: impl IntoIterator<Item = Key>) -> Session<Script> {
        bare(Script::new(TerminalKind::Live, keys, []))
    }

    fn not_empty(text: &str) -> Result<(), &'static str> {
        if text.is_empty() { Err(WRONG) } else { Ok(()) }
    }

    #[test]
    fn secret_never_reaches_the_screen() -> Result<()> {
        let ui = live(chars("739").chain([Key::Enter]));
        let typed = ui.ask(password("Code"))?;
        let raw = ui.terminal().written();
        assert_eq!(*typed, "739");
        assert!(!raw.contains("739"), "the code went out: {raw:?}");
        let printed = without_escapes(&raw);
        assert!(!printed.chars().any(|c| c.is_ascii_digit()), "{printed:?}");
        assert!(screen(&raw).ends_with("◇  Code\n│  ▪▪▪\n│"));
        assert_eq!(ui.terminal().hidden_reads(), 1);
        assert_eq!(ui.terminal().discards(), 1);
        Ok(())
    }

    #[rstest]
    #[case::default_mask(None, "▪▪▪▪")]
    #[case::own_mask(Some('*'), "****")]
    fn mask_shows_one_character_per_typed_one(
        #[case] mask: Option<char>,
        #[case] shown: &str,
    ) -> Result<()> {
        let ui = live(chars("p w!").chain([Key::Enter]));
        let prompt = mask.map_or_else(
            || password("Password"),
            |mask| password("Password").mask(mask),
        );
        assert_eq!(*ui.ask(prompt)?, "p w!");
        let screen = screen(&ui.terminal().written());
        assert!(
            screen.ends_with(&format!("◇  Password\n│  {shown}\n│")),
            "{screen}"
        );
        Ok(())
    }

    #[test]
    fn default_mask_follows_the_symbols() -> Result<()> {
        let ui = live(chars("abc").chain([Key::Enter])).symbols(Symbols::ascii());
        assert_eq!(*ui.ask(password("Code"))?, "abc");
        assert!(screen(&ui.terminal().written()).ends_with("o  Code\n|  ***\n|"));
        Ok(())
    }

    #[test]
    fn editing_keys_work_on_the_mask() -> Result<()> {
        let keys = chars("12345")
            .chain([Key::Backspace, Key::ClearBefore])
            .chain(chars("98"))
            .chain([Key::Enter]);
        let ui = live(keys);
        assert_eq!(*ui.ask(password("Code"))?, "98");
        assert!(screen(&ui.terminal().written()).ends_with("│  ▪▪\n│"));
        Ok(())
    }

    #[test]
    fn the_caret_and_placeholder_are_drawn_not_typed() {
        let ui = live(chars("ab").chain([Key::Left, Key::Cancel]));
        let asked = ui.ask(password("Pass").placeholder("hunter2"));
        assert!(matches!(asked, Err(Error::Cancelled)), "{asked:?}");
        let raw = ui.terminal().written();
        assert!(raw.contains("│  ▏hunter2"), "{raw:?}");
        assert!(raw.contains("│  ▪▏▪"), "{raw:?}");
        assert!(!raw.contains("ab"), "{raw:?}");
    }

    #[test]
    fn a_password_keeps_its_spaces() {
        let keys = [Key::Char(' '), Key::Char('x'), Key::Char(' '), Key::Enter];
        assert_eq!(
            password("P")
                .simulate(keys)
                .ok()
                .as_deref()
                .map(String::as_str),
            Some(" x ")
        );
    }

    #[test]
    fn a_given_secret_is_not_copied_for_the_screen() {
        let secret = Zeroizing::new("hunter2".to_owned());
        let shown = password("P").shown_for(&secret, &Messages::english());
        assert_eq!(shown.as_deref(), Some("*******"));
    }

    #[test]
    fn an_empty_password_is_an_empty_string() -> Result<()> {
        let ui = live([Key::Enter]);
        assert_eq!(*ui.ask(password("Pass"))?, "");
        Ok(())
    }

    #[rstest]
    #[case::keep(OnError::Keep, &["ab", "abc"])]
    #[case::clear(OnError::Clear, &["ab", "c"])]
    fn on_error_keeps_or_clears_the_refused_secret(
        #[case] on_error: OnError,
        #[case] seen: &[&str],
    ) -> Result<()> {
        let keys = chars("ab")
            .chain([Key::Enter])
            .chain(chars("c"))
            .chain([Key::Enter]);
        let ui = live(keys);
        let asked = RefCell::new(Vec::<String>::new());
        let prompt = password("Pass").on_error(on_error).validate(|text| {
            asked.borrow_mut().push(text.to_string());
            if asked.borrow().len() < 2 {
                Err(WRONG)
            } else {
                Ok(())
            }
        });
        ui.ask(prompt)?;
        assert_eq!(*asked.borrow(), seen);
        let raw = ui.terminal().written();
        assert!(raw.contains(&format!("└  {WRONG}")), "{raw:?}");
        assert!(!raw.contains("ab"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn a_refused_secret_is_kept_masked_by_default() -> Result<()> {
        let ui = live(chars("ab").chain([Key::Enter, Key::Enter]));
        let asked = RefCell::new(0_u8);
        let prompt = password("Pass").validate(|_| {
            *asked.borrow_mut() += 1;
            if *asked.borrow() < 2 {
                Err(WRONG)
            } else {
                Ok(())
            }
        });
        ui.ask(prompt)?;
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("◇  Pass\n│  ▪▪\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn cancelled_secret_shows_only_the_mask() {
        let ui = live(chars("739").chain([Key::Cancel]));
        let asked = ui.ask(password("Code"));
        assert!(matches!(asked, Err(Error::Cancelled)), "{asked:?}");
        let raw = ui.terminal().written();
        assert!(!raw.contains("739"), "{raw:?}");
        assert!(screen(&raw).ends_with("■  Code\n│  ▪▪▪\n│"), "{raw:?}");
    }

    #[test]
    fn ctrl_c_is_interrupted() {
        let asked = live([Key::Interrupt]).ask(password("Code"));
        assert!(matches!(asked, Err(Error::Interrupted)), "{asked:?}");
    }

    #[test]
    fn blind_secret_is_read_with_the_echo_off() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["", "123456"]));
        assert_eq!(*ui.ask(password("Code").validate(not_empty))?, "123456");
        assert_eq!(ui.terminal().hidden_reads(), 2);
        let raw = ui.terminal().written();
        assert!(raw.contains(&format!("▲  {WRONG}")), "{raw:?}");
        assert!(!raw.contains("123456"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn blind_secret_fails_when_the_echo_cannot_be_hidden() {
        let script =
            Script::new(TerminalKind::Dumb, [], ["a@b.c"]).without_hidden_echo();
        let err = bare(script).ask(password("Code")).err();
        let said = err.as_ref().map(ToString::to_string).unwrap_or_default();
        assert!(said.starts_with("could not hide the input"), "{err:?}");
    }

    #[test]
    fn a_secret_keeps_its_spaces_for_validate_and_the_answer() {
        let seen = RefCell::new(Vec::new());
        let secret = password("Pass").validate(|text: &str| {
            seen.borrow_mut().push(text.to_owned());
            Ok::<(), String>(())
        });
        let keys = [Key::Char(' '), Key::Char('a'), Key::Char(' '), Key::Enter];
        assert_eq!(
            secret.simulate(keys).ok().as_deref().map(String::as_str),
            Some(" a ")
        );
        assert_eq!(*seen.borrow(), [" a "]);
    }

    #[test]
    fn a_piped_secret_is_read_as_a_line() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Redirected, [], ["s3cret"]));
        assert_eq!(*ui.ask(password("Pass"))?, "s3cret");
        assert_eq!(ui.terminal().discards(), 0);
        let none = bare(Script::new(TerminalKind::Redirected, [], []));
        let asked = none.ask(password("Pass"));
        assert!(matches!(asked, Err(Error::NoInput { .. })), "{asked:?}");
        Ok(())
    }

    #[test]
    fn a_piped_refused_secret_is_not_asked_again() {
        let script = Script::new(TerminalKind::Redirected, [], ["nope", "right"]);
        let err = bare(script)
            .ask(password("Pass").validate(|_| Err::<(), _>(WRONG)))
            .err();
        let said = err.as_ref().map(ToString::to_string).unwrap_or_default();
        assert!(said.starts_with("Pass: that password"), "{err:?}");
    }

    #[test]
    fn the_secret_parses_and_peeks_as_text() {
        let pin = password("Pin").parse::<u16>();
        assert_eq!(
            pin.simulate(chars("1234").chain([Key::Enter])).ok(),
            Some(1234)
        );
        let prompt = password("Pin");
        let state = prompt.start().ok();
        assert_eq!(
            state.as_ref().and_then(|state| prompt.peek(state)),
            Some("")
        );
    }

    #[tokio::test]
    async fn until_asks_again_after_a_wrong_secret_starting_empty() -> Result<()> {
        let keys = chars("bad")
            .chain([Key::Enter])
            .chain(chars("good"))
            .chain([Key::Enter]);
        let ui = live(keys);
        let done = ui
            .ask_until(
                || password("Pass"),
                "Signing in",
                async |secret: &Zeroizing<String>| {
                    if secret.as_str() == "good" {
                        Ok(())
                    } else {
                        Err(WRONG)
                    }
                },
                |_| Some(WRONG.to_owned()),
                NonZeroU8::MIN.saturating_add(1),
            )
            .await?;
        assert_eq!(done, Ok(()));
        let raw = ui.terminal().written();
        assert!(raw.contains(&format!("└  {WRONG}")), "{raw:?}");
        assert!(!raw.contains("good"), "{raw:?}");
        Ok(())
    }

    #[tokio::test]
    async fn until_hands_the_attempt_the_mapped_answer() -> Result<()> {
        use crate::core::ask::Ask as _;

        struct Secret(String);

        let keys = chars("bad")
            .chain([Key::Enter])
            .chain(chars("good"))
            .chain([Key::Enter]);
        let ui = live(keys);
        let done = ui
            .ask_until(
                || {
                    password("Pass").map(|typed: Zeroizing<String>| {
                        Secret(typed.as_str().to_uppercase())
                    })
                },
                "Signing in",
                async |secret: &Secret| {
                    if secret.0 == "GOOD" {
                        Ok(())
                    } else {
                        Err(WRONG)
                    }
                },
                |_| Some(WRONG.to_owned()),
                NonZeroU8::MIN.saturating_add(1),
            )
            .await?;
        assert_eq!(done, Ok(()));
        let raw = ui.terminal().written();
        assert!(raw.contains(&format!("└  {WRONG}")), "{raw:?}");
        Ok(())
    }

    #[cfg(feature = "secrecy")]
    #[test]
    fn secret_answers_a_secret_string_with_the_typed_text() -> Result<()> {
        use secrecy::ExposeSecret as _;

        let ui = live(chars("hunter2").chain([Key::Enter]));
        let secret = ui.ask(password("Pass").secret())?;
        assert_eq!(secret.expose_secret(), "hunter2");
        Ok(())
    }

    #[cfg(feature = "secrecy")]
    #[test]
    fn secret_copies_the_typed_text_exactly() -> Result<()> {
        use secrecy::ExposeSecret as _;

        let typed = "p\u{e4}ss w0rd ".repeat(40);
        let ui = live(chars(&typed).chain([Key::Enter]));
        let secret = ui.ask(password("Pass").secret())?;
        assert_eq!(secret.expose_secret(), typed);
        Ok(())
    }

    #[cfg(feature = "secrecy")]
    #[tokio::test]
    async fn secret_works_with_ask_until() -> Result<()> {
        use secrecy::ExposeSecret as _;

        let ui = live(chars("good").chain([Key::Enter]));
        let done = ui
            .ask_until(
                || password("Pass").secret(),
                "Signing in",
                async |secret: &secrecy::SecretString| {
                    if secret.expose_secret() == "good" {
                        Ok(())
                    } else {
                        Err(WRONG)
                    }
                },
                |_| Some(WRONG.to_owned()),
                NonZeroU8::MIN,
            )
            .await?;
        assert_eq!(done, Ok(()));
        Ok(())
    }
}
