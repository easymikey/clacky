//! The yes or no question.

use std::{cell::Cell, fmt::Display};

use crate::{
    core::{
        ask::{Ask, Frame, Key, Next, Peek, Phase, Prefill, Prompt},
        error::{Error, Result},
        keys::Printable,
        session::Session,
    },
    prompts::choice::{radio_row, rail},
    render::messages::{Messages, Refusal},
    terminal::Terminal,
    values::line::Cancellation,
};

/// What Enter answers a [`Confirm`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// The answer is yes.
    Yes,
    /// The answer is no.
    No,
}

impl From<bool> for Reply {
    fn from(yes: bool) -> Self {
        if yes { Self::Yes } else { Self::No }
    }
}

impl Reply {
    const fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// Asks yes or no; Enter takes Yes unless told otherwise, and so does
/// `Defaults::Take`.
///
/// ```no_run
/// use clacky::prelude::*;
/// # fn main() -> clacky::core::error::Result<()> {
/// let ui = Session::detect()?;
/// let proceed: bool = ui.ask(
///     confirm("Deploy to production?")
///         .default(Reply::No)
///         .labels("Deploy", "Not now"),
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// A No that should end the form is [`or_cancel`](Confirm::or_cancel).
#[must_use]
pub fn confirm(message: impl Into<String>) -> Confirm {
    Confirm {
        message: message.into(),
        default: Reply::Yes,
        yes: None,
        no: None,
        layout: Layout::Inline,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Inline,
    Vertical,
}

/// A yes or no question; see [`confirm`].
#[derive(Debug)]
pub struct Confirm {
    message: String,
    default: Reply,
    yes: Option<String>,
    no: Option<String>,
    layout: Layout,
}

impl Confirm {
    /// The answer Enter takes, and `Defaults::Take`; Yes by default.
    #[must_use]
    pub fn default(self, reply: Reply) -> Self {
        Self {
            default: reply,
            ..self
        }
    }

    /// Ends the form on No: the question is shown cancelled, then `message` as
    /// [`Session::cancel`] shows it, and the answer is
    /// [`Error::Cancelled`]. Yes gives `()`, so the code after it is the yes
    /// branch.
    ///
    /// ```no_run
    /// use clacky::prelude::*;
    /// # fn main() -> clacky::core::error::Result<()> {
    /// let ui = Session::detect()?;
    /// ui.ask(confirm("Create it?").or_cancel("Nothing created"))?;
    /// ui.outro("Created")
    /// # }
    /// ```
    #[must_use]
    pub fn or_cancel(self, message: impl Display) -> OrCancel {
        OrCancel {
            confirm: self,
            message: message.to_string(),
        }
    }

    /// The words that stand for yes and no.
    #[must_use]
    pub fn labels(self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        Self {
            yes: Some(yes.into()),
            no: Some(no.into()),
            ..self
        }
    }

    /// Puts Yes and No on two rows; Up and Down move between them.
    #[must_use]
    pub fn vertical(self) -> Self {
        Self {
            layout: Layout::Vertical,
            ..self
        }
    }

    fn label<'a>(&'a self, yes: bool, messages: &'a Messages) -> &'a str {
        if yes {
            self.yes.as_deref().unwrap_or(&messages.yes)
        } else {
            self.no.as_deref().unwrap_or(&messages.no)
        }
    }
}

impl Confirm {
    fn answer(&self, line: &str, messages: &Messages) -> Option<bool> {
        let line = line.trim().to_lowercase();
        let [yes, no] =
            [true, false].map(|yes| self.label(yes, messages).to_lowercase());
        let mut letters = line.chars();
        let letter = letters.next().filter(|_| letters.next().is_none());
        if line == yes || line == "yes" {
            Some(true)
        } else if line == no || line == "no" {
            Some(false)
        } else {
            letter.and_then(|letter| self.initial(letter, messages))
        }
    }

    /// `y` and `n` always mean yes and no; a label letter counts only when
    /// exactly one label starts with it.
    fn initial(&self, letter: char, messages: &Messages) -> Option<bool> {
        let [yes, no] =
            [true, false].map(|yes| self.label(yes, messages).to_lowercase());
        match letter {
            'y' => Some(true),
            'n' => Some(false),
            _ => match (yes.starts_with(letter), no.starts_with(letter)) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            },
        }
    }
}

impl Prompt for Confirm {
    type Output = bool;
    type State = bool;

    fn message(&self) -> &str {
        &self.message
    }

    fn start(&self) -> Result<bool> {
        Ok(self.default.is_yes())
    }

    /// Label initials win over the key map by becoming `Y` and `N`, which no
    /// binding touches.
    fn spelled(&self, key: Key, messages: &Messages) -> Key {
        let Key::Char(c) = key else { return key };
        c.to_lowercase()
            .next()
            .and_then(|letter| self.initial(letter, messages))
            .map_or(key, |yes| Key::Char(if yes { 'Y' } else { 'N' }))
    }

    fn update(&self, yes: bool, key: Key) -> Next<bool> {
        match key {
            Key::Left | Key::Right | Key::Tab => Next::Editing(!yes),
            Key::Up | Key::Down if self.layout == Layout::Vertical => {
                Next::Editing(!yes)
            }
            Key::Char('y' | 'Y') => Next::Submit(true),
            Key::Char('n' | 'N') => Next::Submit(false),
            Key::Enter => Next::Submit(yes),
            Key::Interrupt => Next::Quit(yes, Cancellation::Interrupt),
            Key::Cancel | Key::EndOfInput => Next::Quit(yes, Cancellation::Cancel),
            Key::Char(_)
            | Key::Backspace
            | Key::Delete
            | Key::Up
            | Key::Down
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::ClearBefore
            | Key::ClearAfter
            | Key::DeleteWord
            | Key::Ignored => Next::Editing(yes),
        }
    }

    fn view(&self, yes: &bool, phase: Phase<'_>, frame: &mut Frame) {
        let skin = frame.skin();
        let theme = skin.theme();
        match phase {
            Phase::Active | Phase::Warned(_) => {
                let option = |on: bool, answer: bool| {
                    radio_row(skin, on, self.label(answer, frame.messages()), None)
                };
                let rows: Vec<String> = match self.layout {
                    Layout::Inline => vec![format!(
                        "{}  {} {} {}",
                        rail(skin, phase),
                        option(*yes, true),
                        skin.paint(theme.hint, "/"),
                        option(!*yes, false)
                    )],
                    Layout::Vertical => [true, false]
                        .map(|answer| {
                            format!(
                                "{}  {}",
                                rail(skin, phase),
                                option(*yes == answer, answer)
                            )
                        })
                        .into(),
                };
                for row in rows {
                    frame.row(row);
                }
            }
            Phase::Submitted | Phase::Cancelled => {
                let label = self.label(*yes, frame.messages()).to_owned();
                frame.row(label);
            }
        }
    }

    fn finish(self, yes: bool) -> Result<bool> {
        Ok(yes)
    }

    fn by_default(&self) -> Option<bool> {
        Some(self.default.is_yes())
    }

    fn printable(&self) -> Printable {
        Printable::Bound
    }

    fn typed(&self, line: &str, messages: &Messages) -> Vec<Key> {
        self.answer(line, messages)
            .map(|yes| Key::Char(if yes { 'y' } else { 'n' }))
            .into_iter()
            .chain([Key::Enter])
            .collect()
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        (!line.trim().is_empty() && self.answer(line, messages).is_none())
            .then_some(Refusal::NoOption)
    }
}

/// A [`Confirm`] whose No ends the form; build it with [`Confirm::or_cancel`].
#[derive(Debug)]
pub struct OrCancel {
    confirm: Confirm,
    message: String,
}

impl Ask for OrCancel {
    type Output = ();

    fn run<T: Terminal>(self, session: &Session<T>) -> Result<()> {
        let declined = Cell::new(false);
        let question = Declining {
            confirm: self.confirm,
            declined: &declined,
        };
        match session.drive(question) {
            Ok(_) => Ok(()),
            Err(Error::Cancelled) if declined.get() => {
                session.cancel(&self.message)?;
                Err(Error::Cancelled)
            }
            Err(error) => Err(error),
        }
    }
}

/// The question of an [`OrCancel`]: a No quits, and says so in `declined`, so
/// Esc stays apart from it.
struct Declining<'a> {
    confirm: Confirm,
    declined: &'a Cell<bool>,
}

impl Prompt for Declining<'_> {
    type Output = bool;
    type State = bool;

    fn message(&self) -> &str {
        self.confirm.message()
    }

    fn start(&self) -> Result<bool> {
        self.confirm.start()
    }

    fn spelled(&self, key: Key, messages: &Messages) -> Key {
        self.confirm.spelled(key, messages)
    }

    fn update(&self, yes: bool, key: Key) -> Next<bool> {
        match self.confirm.update(yes, key) {
            Next::Submit(false) => {
                self.declined.set(true);
                Next::Quit(false, Cancellation::Cancel)
            }
            next @ (Next::Editing(_)
            | Next::Refuse(..)
            | Next::Submit(true)
            | Next::Quit(..)) => next,
        }
    }

    fn view(&self, yes: &bool, phase: Phase<'_>, frame: &mut Frame) {
        self.confirm.view(yes, phase, frame);
    }

    fn finish(self, yes: bool) -> Result<bool> {
        self.confirm.finish(yes)
    }

    fn by_default(&self) -> Option<bool> {
        self.confirm.by_default()
    }

    fn printable(&self) -> Printable {
        self.confirm.printable()
    }

    fn typed(&self, line: &str, messages: &Messages) -> Vec<Key> {
        self.confirm.typed(line, messages)
    }

    fn misread(&self, line: &str, messages: &Messages) -> Option<Refusal> {
        self.confirm.misread(line, messages)
    }
}

impl Peek for Confirm {
    type Peeked<'a> = bool;

    fn peek<'a>(&'a self, yes: &'a bool) -> Option<bool> {
        Some(*yes)
    }
}

impl Prefill for Confirm {
    fn shown_for(&self, answer: &bool, messages: &Messages) -> Option<String> {
        Some(self.label(*answer, messages).to_owned())
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use rstest::rstest;

    use super::{Confirm, Reply, confirm};
    use crate::{
        core::{
            ask::{Frame, Key, Phase, Prompt},
            error::{Error, Result},
            settings::Defaults,
        },
        fake::{Script, screen, testing::bare},
        render::{color::ColorChoice, messages::Messages},
        terminal::TerminalKind,
    };

    fn rendered(prompt: &Confirm, yes: bool, phase: Phase<'_>) -> Vec<String> {
        let mut frame = Frame::new(ColorChoice::Plain, Some(80));
        prompt.view(&yes, phase, &mut frame);
        frame.rows().to_vec()
    }

    #[rstest]
    #[case::no_by_default(Reply::No, vec![Key::Enter], Some(false))]
    #[case::yes_by_default(Reply::Yes, vec![Key::Enter], Some(true))]
    #[case::y_submits(Reply::No, vec![Key::Char('y')], Some(true))]
    #[case::capital_y(Reply::No, vec![Key::Char('Y')], Some(true))]
    #[case::n_submits(Reply::Yes, vec![Key::Char('n')], Some(false))]
    #[case::capital_n(Reply::Yes, vec![Key::Char('N')], Some(false))]
    #[case::left_flips(Reply::No, vec![Key::Left, Key::Enter], Some(true))]
    #[case::right_flips(Reply::Yes, vec![Key::Right, Key::Enter], Some(false))]
    #[case::tab_flips(Reply::No, vec![Key::Tab, Key::Enter], Some(true))]
    #[case::h_flips(Reply::No, vec![Key::Char('h'), Key::Enter], Some(true))]
    #[case::l_flips(Reply::Yes, vec![Key::Char('l'), Key::Enter], Some(false))]
    #[case::flip_twice(Reply::No, vec![Key::Tab, Key::Tab, Key::Enter], Some(false))]
    #[case::other_keys_do_nothing(
        Reply::No,
        vec![Key::Char('x'), Key::Up, Key::Down, Key::Backspace, Key::Ignored, Key::Enter],
        Some(false)
    )]
    #[case::keys_run_out(Reply::Yes, vec![Key::Tab], None)]
    fn keys_move_or_answer(
        #[case] default: Reply,
        #[case] keys: Vec<Key>,
        #[case] expected: Option<bool>,
    ) {
        let result = confirm("Go?").default(default).simulate(keys);
        match expected {
            Some(yes) => assert_eq!(result.ok(), Some(yes)),
            None => assert!(matches!(result, Err(Error::NoInput { .. }))),
        }
    }

    #[rstest]
    #[case::ctrl_c(Key::Interrupt, true)]
    #[case::escape(Key::Cancel, false)]
    #[case::ctrl_d(Key::EndOfInput, false)]
    fn quitting_keys_stop_the_question(#[case] key: Key, #[case] interrupt: bool) {
        let result = confirm("Go?").simulate([key]);
        if interrupt {
            assert!(matches!(result, Err(Error::Interrupted)), "{result:?}");
        } else {
            assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        }
    }

    #[rstest]
    #[case::no(false, "│  ○ Yes / ● No", "No")]
    #[case::yes(true, "│  ● Yes / ○ No", "Yes")]
    fn view_marks_the_highlighted_answer(
        #[case] yes: bool,
        #[case] row: &str,
        #[case] label: &str,
    ) {
        let prompt = confirm("Go?");
        assert_eq!(rendered(&prompt, yes, Phase::Active), [row]);
        assert_eq!(rendered(&prompt, yes, Phase::Submitted), [label]);
        assert_eq!(rendered(&prompt, yes, Phase::Cancelled), [label]);
    }

    #[rstest]
    #[case::no(false, ["│  ○ Yes", "│  ● No"], "No")]
    #[case::yes(true, ["│  ● Yes", "│  ○ No"], "Yes")]
    fn vertical_view_stacks_the_answers(
        #[case] yes: bool,
        #[case] rows: [&str; 2],
        #[case] label: &str,
    ) {
        let prompt = confirm("Go?").vertical();
        assert_eq!(rendered(&prompt, yes, Phase::Active), rows);
        assert_eq!(rendered(&prompt, yes, Phase::Submitted), [label]);
        assert_eq!(rendered(&prompt, yes, Phase::Cancelled), [label]);
    }

    #[rstest]
    #[case::up(Key::Up, Some(false))]
    #[case::down(Key::Down, Some(false))]
    #[case::tab(Key::Tab, Some(false))]
    fn vertical_arrows_move(#[case] key: Key, #[case] expected: Option<bool>) {
        let result = confirm("Go?").vertical().simulate(vec![key, Key::Enter]);
        assert_eq!(result.ok(), expected);
    }

    #[test]
    fn inline_ignores_up_and_down() {
        let result = confirm("Go?").simulate(vec![Key::Up, Key::Down, Key::Enter]);
        assert_eq!(result.ok(), Some(true));
    }

    #[rstest]
    #[case::enter_takes_yes(vec![Key::Enter], "│  Yes\n")]
    #[case::left_flips(vec![Key::Left, Key::Enter], "│  No\n")]
    fn confirm_answers_with_the_highlight(
        #[case] keys: Vec<Key>,
        #[case] row: &str,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, keys, []));
        let yes = ui.ask(confirm("Go?"))?;
        let shown = screen(&ui.terminal().written());
        assert_eq!(yes, row.contains("Yes"));
        assert!(shown.ends_with(&format!("◇  Go?\n{row}│")), "{shown}");
        Ok(())
    }

    #[rstest]
    #[case::own_yes(Key::Char('d'), true)]
    #[case::own_yes_capital(Key::Char('D'), true)]
    #[case::own_no(Key::Char('s'), false)]
    #[case::y_still_works(Key::Char('y'), true)]
    fn a_live_terminal_takes_the_first_letter_of_a_label(
        #[case] key: Key,
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [key], []));
        let yes = ui.ask(
            confirm("Go?")
                .default(Reply::No)
                .labels("Deploy", "Skip it"),
        )?;
        assert_eq!(yes, expected);
        Ok(())
    }

    #[rstest]
    #[case::n_is_no_even_for_a_yes_label_on_n(Key::Char('n'), false)]
    #[case::y_is_yes(Key::Char('y'), true)]
    #[case::own_no(Key::Char('k'), false)]
    fn n_and_y_keep_their_meaning_over_label_letters(
        #[case] key: Key,
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [key], []));
        let yes = ui.ask(
            confirm("Delete all?")
                .default(Reply::No)
                .labels("Nuke it", "Keep"),
        )?;
        assert_eq!(yes, expected);
        Ok(())
    }

    #[test]
    fn a_letter_both_labels_start_with_answers_nothing() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["s", "skip"]));
        let yes = ui.ask(confirm("Go?").labels("Save", "Skip"))?;
        assert!(!yes);
        Ok(())
    }

    #[test]
    fn a_live_terminal_takes_the_first_letter_of_the_messages() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Char('j')], []))
            .messages(Messages::english().yes("Ja").no("Nein"));
        assert!(ui.ask(confirm("Go?").default(Reply::No))?);
        Ok(())
    }

    #[test]
    fn default_and_labels_change_the_question() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Enter], []));
        let yes = ui.ask(
            confirm("Go?")
                .default(Reply::No)
                .labels("Deploy", "Not now"),
        )?;
        let written = ui.terminal().written();
        assert!(!yes);
        assert!(written.contains("○ Deploy / ● Not now"), "{written:?}");
        assert!(
            screen(&written).ends_with("◇  Go?\n│  Not now\n│"),
            "{written:?}"
        );
        Ok(())
    }

    #[rstest]
    #[case::yes(vec![], "│  Yes\n")]
    #[case::no(vec![Key::Left], "│  No\n")]
    fn cancelled_confirm_keeps_the_highlight(
        #[case] keys: Vec<Key>,
        #[case] row: &str,
    ) {
        let strokes = keys.into_iter().chain([Key::Cancel]);
        let ui = bare(Script::new(TerminalKind::Live, strokes, []));
        assert!(matches!(ui.ask(confirm("Go?")), Err(Error::Cancelled)));
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with(&format!("■  Go?\n{row}│")), "{shown}");
    }

    #[rstest]
    #[case::dumb_letter(TerminalKind::Dumb, &["y"], true)]
    #[case::dumb_blank_takes_the_highlight(TerminalKind::Dumb, &[""], true)]
    fn a_typed_line_answers_without_keys(
        #[case] kind: TerminalKind,
        #[case] lines: &'static [&'static str],
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(kind, [], lines.iter().copied()));
        assert_eq!(ui.ask(confirm("Go?"))?, expected);
        Ok(())
    }

    fn german() -> Messages {
        Messages::english().yes("Ja").no("Nein")
    }

    #[rstest]
    #[case::english_word("yes", Reply::No, Messages::english(), true)]
    #[case::english_word_no("NO", Reply::Yes, Messages::english(), false)]
    #[case::own_yes_word("ja", Reply::No, german(), true)]
    #[case::own_yes_loud("JA", Reply::No, german(), true)]
    #[case::own_yes_letter("j", Reply::No, german(), true)]
    #[case::own_no_word("Nein", Reply::Yes, german(), false)]
    #[case::own_no_letter("n", Reply::Yes, german(), false)]
    #[case::y_still_works("y", Reply::No, german(), true)]
    fn a_typed_line_answers_with_the_messages(
        #[case] line: &'static str,
        #[case] default: Reply,
        #[case] messages: Messages,
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], [line])).messages(messages);
        assert_eq!(ui.ask(confirm("Go?").default(default))?, expected);
        Ok(())
    }

    #[rstest]
    #[case::word("deploy", Reply::No, true)]
    #[case::phrase("Not Now", Reply::Yes, false)]
    #[case::letter("d", Reply::No, true)]
    fn a_typed_line_answers_with_the_own_labels(
        #[case] line: &'static str,
        #[case] default: Reply,
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], [line]));
        let prompt = confirm("Go?").default(default).labels("Deploy", "Not now");
        assert_eq!(ui.ask(prompt)?, expected);
        Ok(())
    }

    #[test]
    fn a_typed_line_that_answers_nothing_is_asked_again() -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["maybe", "ja"]))
            .messages(german());
        assert!(ui.ask(confirm("Go?").default(Reply::No))?);
        let written = ui.terminal().written();
        assert!(
            written.contains(&*Messages::english().pick_an_option),
            "{written:?}"
        );
        Ok(())
    }

    #[test]
    fn piped_input_is_no_input() {
        let ui = bare(Script::new(TerminalKind::Redirected, [], ["y"]));
        assert!(matches!(ui.ask(confirm("Go?")), Err(Error::NoInput { .. })));
    }

    #[rstest]
    #[case::built_in_default_is_yes(confirm("Go?"), true)]
    #[case::yes_stated(confirm("Go?").default(Reply::Yes), true)]
    #[case::no_changes_it(confirm("Go?").default(Reply::No), false)]
    fn take_answers_with_the_default(
        #[case] prompt: Confirm,
        #[case] expected: bool,
    ) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [], [])).defaults(Defaults::Take);
        assert_eq!(ui.ask(prompt)?, expected);
        Ok(())
    }

    #[test]
    fn no_ends_the_form_with_the_cancelled_question_and_the_line() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Char('n')], []));
        let result = ui.ask(confirm("Go?").or_cancel("Nothing created"));
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        let shown = screen(&ui.terminal().written());
        assert!(
            shown.ends_with("■  Go?\n│  No\n│\n└  Nothing created"),
            "{shown}"
        );
    }

    #[rstest]
    #[case::enter(Key::Enter)]
    #[case::y(Key::Char('y'))]
    fn yes_goes_on_without_a_line(#[case] key: Key) -> Result<()> {
        let ui = bare(Script::new(TerminalKind::Live, [key], []));
        ui.ask(confirm("Go?").or_cancel("Nothing created"))?;
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("◇  Go?\n│  Yes\n│"), "{shown}");
        Ok(())
    }

    #[test]
    fn escape_cancels_without_the_line() {
        let ui = bare(Script::new(TerminalKind::Live, [Key::Cancel], []));
        let result = ui.ask(confirm("Go?").or_cancel("Nothing created"));
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        let shown = screen(&ui.terminal().written());
        assert!(!shown.contains("Nothing created"), "{shown}");
    }

    #[test]
    fn a_typed_no_ends_the_form_too() {
        let ui = bare(Script::new(TerminalKind::Dumb, [], ["n"]));
        let result = ui.ask(confirm("Go?").or_cancel("Nothing created"));
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        let shown = screen(&ui.terminal().written());
        assert!(shown.ends_with("└  Nothing created"), "{shown}");
    }

    #[rstest]
    #[case::yes(Reply::Yes, true)]
    #[case::no(Reply::No, false)]
    fn take_continues_on_yes_and_cancels_on_no(
        #[case] default: Reply,
        #[case] goes_on: bool,
    ) {
        let ui = bare(Script::new(TerminalKind::Live, [], [])).defaults(Defaults::Take);
        let result =
            ui.ask(confirm("Go?").default(default).or_cancel("Nothing created"));
        assert_eq!(result.is_ok(), goes_on, "{result:?}");
        let shown = screen(&ui.terminal().written());
        assert_eq!(shown.contains("└  Nothing created"), !goes_on, "{shown}");
    }
}
