//! `#[derive(Choices)]` and `#[derive(Form)]` end to end, on the scripted
//! terminal.

#![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

use clacky::{
    core::{
        ask::{CancelAsNone, Key, Prompt},
        askable::Answer,
        error::{Error, Result},
        settings::{Defaults, Settings},
    },
    fake::{Script, screen},
    prelude::Session,
    prompts::choice::Choice,
    render::{color::ColorChoice, messages::Messages},
    terminal::TerminalKind,
};

#[derive(Debug, PartialEq, clacky::Choices)]
enum Kind {
    #[ask("Library")]
    Lib,
    #[ask("Binary", hint = "recommended")]
    Bin,
    HttpServer,
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Setup {
    #[ask("Kind")]
    kind: Kind,
    other_kind: Kind,
}

fn session(keys: impl IntoIterator<Item = Key>) -> Session<Script> {
    Session::new(
        Script::new(TerminalKind::Live, keys, []),
        Settings::new(ColorChoice::Plain),
    )
}

#[test]
fn an_enum_is_a_select_over_its_variants() -> Result<()> {
    assert_eq!(
        Kind::prompt("Kind").simulate([Key::Down, Key::Enter])?,
        Kind::Bin
    );
    assert_eq!(Kind::prompt("Kind").simulate([Key::Enter])?, Kind::Lib);
    Ok(())
}

#[test]
fn labels_and_hints_are_shown() -> Result<()> {
    let ui = session([Key::Down, Key::Enter]);
    let kind = ui.ask(Kind::prompt("Kind"))?;
    assert_eq!(kind, Kind::Bin);
    let shown = ui.terminal().written();
    assert!(shown.contains("Library"), "{shown}");
    assert!(shown.contains("Binary"), "{shown}");
    assert!(shown.contains("recommended"), "{shown}");
    assert!(shown.contains("Http server"), "{shown}");
    Ok(())
}

#[test]
fn a_struct_asks_its_fields_in_order() -> Result<()> {
    let ui = session([Key::Down, Key::Enter, Key::Down, Key::Down, Key::Enter]);
    let setup = ui.ask(Setup::form())?;
    assert_eq!(
        setup,
        Setup {
            kind: Kind::Bin,
            other_kind: Kind::HttpServer
        }
    );
    let shown = screen(&ui.terminal().written());
    let first = shown.find("Kind").unwrap_or(usize::MAX);
    let second = shown.find("Other kind").unwrap_or(usize::MAX);
    assert!(first < second, "{shown}");
    Ok(())
}

#[test]
fn a_struct_stops_at_the_first_field_the_person_leaves() -> Result<()> {
    let ui = session([Key::Enter, Key::Cancel]);
    assert!(ui.ask(Setup::form()).cancel_as_none()?.is_none());
    Ok(())
}

fn typed(text: &str) -> impl Iterator<Item = Key> + '_ {
    text.chars().map(Key::Char)
}

fn long_enough(text: &str) -> std::result::Result<(), String> {
    if text.len() >= 3 {
        Ok(())
    } else {
        Err("at least 3 letters".to_owned())
    }
}

#[derive(Debug, PartialEq)]
struct Level(u8);

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for Level {
    type Err = String;

    fn from_str(text: &str) -> std::result::Result<Self, String> {
        text.parse().map(Self).map_err(|_| "not a level".to_owned())
    }
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Fields {
    #[ask("Name", placeholder = "my-app", validate = long_enough)]
    name: String,
    #[ask("Port", default = 8080)]
    port: u16,
    #[ask("Ratio", default = 0.5)]
    ratio: f64,
    #[ask("Install?", default = false)]
    install: bool,
    #[ask("Nick")]
    nick: Option<String>,
    #[ask("Retries")]
    retries: Option<u8>,
    #[ask("Kind")]
    kind: Option<Kind>,
    #[ask("Kinds", optional)]
    kinds: Vec<Kind>,
    #[ask("Level", parse, default = 3)]
    level: Level,
    #[ask(skip)]
    ignored: u32,
}

#[test]
fn defaults_blanks_and_skips_fill_the_answers() -> Result<()> {
    let keys: Vec<Key> = typed("ab")
        .chain([Key::Enter]) // too short: refused
        .chain(typed("c"))
        .chain([Key::Enter]) // name
        .chain([Key::Enter]) // port: default
        .chain([Key::Enter]) // ratio: default
        .chain([Key::Enter]) // install: default, No
        .chain([Key::Enter]) // nick: blank
        .chain(typed("2"))
        .chain([Key::Enter]) // retries
        .chain([Key::Down, Key::Down, Key::Down, Key::Enter]) // kind: Skip
        .chain([Key::Enter]) // kinds: none, optional
        .chain([Key::Enter]) // level: default
        .collect();
    let ui = session(keys);
    let got = ui.ask(Fields::form())?;
    assert_eq!(
        got,
        Fields {
            name: "abc".to_owned(),
            port: 8080,
            ratio: 0.5,
            install: false,
            nick: None,
            retries: Some(2),
            kind: None,
            kinds: Vec::new(),
            level: Level(3),
            ignored: 0,
        }
    );
    let shown = ui.terminal().written();
    assert!(shown.contains("at least 3 letters"), "{shown}");
    assert!(shown.contains("my-app"), "{shown}");
    assert!(shown.contains("Skip"), "{shown}");
    Ok(())
}

#[test]
fn typed_answers_win_over_defaults() -> Result<()> {
    let keys: Vec<Key> = typed("abc")
        .chain([Key::Enter])
        .chain(typed("9000"))
        .chain([Key::Enter])
        .chain(typed("1.5"))
        .chain([Key::Enter])
        .chain([Key::Char('y')])
        .chain(typed("zed"))
        .chain([Key::Enter])
        .chain([Key::Enter])
        .chain([Key::Enter]) // kind: the first variant
        .chain([Key::Char(' '), Key::Down, Key::Char(' '), Key::Enter])
        .chain(typed("7"))
        .chain([Key::Enter])
        .collect();
    let got = session(keys).ask(Fields::form())?;
    assert_eq!(got.port, 9000);
    assert!((got.ratio - 1.5).abs() < f64::EPSILON);
    assert!(got.install);
    assert_eq!(got.nick.as_deref(), Some("zed"));
    assert_eq!(got.retries, None);
    assert_eq!(got.kind, Some(Kind::Lib));
    assert_eq!(got.kinds, vec![Kind::Lib, Kind::Bin]);
    assert_eq!(got.level, Level(7));
    Ok(())
}

#[test]
fn a_number_that_is_not_typed_is_refused() {
    let ui = session(typed("x").chain([Key::Enter]));
    assert!(ui.ask(<u16 as Answer>::prompt("Port")).is_err());
    let shown = ui.terminal().written();
    assert!(shown.contains("Enter a number"), "{shown}");
}

#[test]
fn a_multiselect_without_optional_needs_a_pick() {
    let ui = session([Key::Enter]);
    assert!(ui.ask(<Vec<Kind> as Answer>::prompt("Kinds")).is_err());
}

#[test]
fn a_date_takes_its_default() -> Result<()> {
    use clacky::{core::askable::Fillable, values::date::Date};
    let day = Date::new(2024, 5, 6);
    let got = <Date as Fillable>::filled("Day", None, day).simulate([Key::Enter])?;
    assert_eq!(Some(got), day);
    Ok(())
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Server {
    name: String,
    #[ask("Port", default = 8080)]
    port: u16,
    nick: Option<String>,
    kind: Kind,
    #[ask(skip)]
    ignored: u32,
}

#[test]
fn given_answers_are_taken_and_the_rest_asked() -> Result<()> {
    let ui = session([Key::Enter, Key::Down, Key::Enter]);
    let got = ui.ask(
        Server::form()
            .name(Some("api".to_owned()))
            .nick(Some(Some("bob".to_owned()))),
    )?;
    assert_eq!(
        got,
        Server {
            name: "api".to_owned(),
            port: 8080,
            nick: Some("bob".to_owned()),
            kind: Kind::Bin,
            ignored: 0,
        }
    );
    let shown = ui.terminal().written();
    assert!(shown.contains("api"), "{shown}");
    assert!(shown.contains("bob"), "{shown}");
    assert!(shown.contains("Port"), "{shown}");
    Ok(())
}

#[test]
fn nothing_given_asks_everything() -> Result<()> {
    let keys: Vec<Key> = typed("x")
        .chain([Key::Enter, Key::Enter, Key::Enter, Key::Enter])
        .collect();
    let got = session(keys).ask(Server::form())?;
    assert_eq!(got.name, "x");
    assert_eq!(got.nick, None);
    Ok(())
}

#[test]
fn a_setter_given_none_asks() -> Result<()> {
    let keys: Vec<Key> = typed("x")
        .chain([Key::Enter, Key::Enter, Key::Enter, Key::Enter])
        .collect();
    let got = session(keys).ask(Server::form().name(None).port(None))?;
    assert_eq!(got.name, "x");
    Ok(())
}

#[derive(Debug, PartialEq, clacky::Form)]
#[ask(form_name = "Known")]
struct Counted {
    #[ask(default = 3)]
    count: u8,
    retries: u8,
}

fn defaulting() -> Session<Script> {
    let settings = Settings::new(ColorChoice::Plain).defaults(Defaults::Take);
    Session::new(Script::new(TerminalKind::Live, [], []), settings)
}

#[test]
fn defaults_fill_the_rest_and_a_missing_default_is_an_error() -> Result<()> {
    let ui = defaulting();
    let got = ui.ask(Counted::form().retries(Some(2)))?;
    assert_eq!(
        got,
        Counted {
            count: 3,
            retries: 2
        }
    );
    let missing = defaulting().ask(Counted::form());
    assert!(
        matches!(missing, Err(Error::NoDefault { .. })),
        "{missing:?}"
    );
    Ok(())
}

#[derive(Debug, clacky::Choices)]
enum Plain {
    First,
    Second,
}

#[derive(Debug, clacky::Form)]
struct Prefilled {
    plain: Plain,
    maybe: Option<Plain>,
    several: Vec<Plain>,
}

#[test]
fn a_derived_enum_without_partial_eq_is_prefilled_by_label() -> Result<()> {
    let ui = session([]);
    let got = ui.ask(
        Prefilled::form()
            .plain(Some(Plain::Second))
            .maybe(Some(Some(Plain::First)))
            .several(Some(vec![Plain::First, Plain::Second])),
    )?;
    assert!(matches!(got.plain, Plain::Second), "{got:?}");
    assert!(matches!(got.maybe, Some(Plain::First)), "{got:?}");
    assert_eq!(got.several.len(), 2);
    let shown = screen(&ui.terminal().written());
    assert!(shown.contains("Second"), "{shown}");
    assert!(shown.contains("First, Second"), "{shown}");
    Ok(())
}

#[test]
fn a_skipped_option_is_prefilled_as_skip() -> Result<()> {
    let ui = session([]);
    let got = ui.ask(
        Prefilled::form()
            .plain(Some(Plain::First))
            .maybe(Some(None))
            .several(Some(vec![Plain::Second])),
    )?;
    assert!(got.maybe.is_none(), "{got:?}");
    let shown = screen(&ui.terminal().written());
    assert!(shown.contains("Skip"), "{shown}");
    Ok(())
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Job {
    run: bool,
    map: u8,
    abort: bool,
    timeout: u64,
}

#[test]
fn setters_named_like_ask_methods_get_with_first() -> Result<()> {
    let ui = session([]);
    let got = ui.ask(
        Job::form()
            .with_run(Some(true))
            .with_map(Some(3))
            .with_abort(Some(false))
            .with_timeout(Some(30)),
    )?;
    assert_eq!(
        got,
        Job {
            run: true,
            map: 3,
            abort: false,
            timeout: 30
        }
    );
    Ok(())
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Nothing {
    kind: Kind,
    maybe_kind: Option<Kind>,
    nick: Option<String>,
    retries: Option<u8>,
    #[ask(optional)]
    tools: Vec<Kind>,
}

#[test]
fn under_take_an_empty_answer_is_the_default_where_nothing_is_fine() -> Result<()> {
    let got = defaulting().ask(Nothing::form())?;
    assert_eq!(
        got,
        Nothing {
            kind: Kind::Lib,
            maybe_kind: None,
            nick: None,
            retries: None,
            tools: Vec::new(),
        }
    );
    Ok(())
}

#[derive(Debug, clacky::Form)]
#[expect(dead_code, reason = "asked, never read")]
struct Needs {
    #[ask(optional)]
    tools: Vec<Kind>,
    kinds: Vec<Kind>,
}

#[test]
fn under_take_a_required_vec_has_no_default() {
    let got = defaulting().ask(Needs::form());
    assert!(matches!(got, Err(Error::NoDefault { .. })), "{got:?}");
}

#[derive(Debug, PartialEq)]
enum Plan {
    Free,
    Pro,
}

impl clacky::core::askable::Choices for Plan {
    fn choices() -> Vec<Choice<Self>> {
        vec![
            Choice::new(Self::Free, "Free"),
            Choice::disabled(Self::Pro, "Pro"),
        ]
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Pro => "Pro",
        }
    }
}

#[test]
fn an_option_field_keeps_a_disabled_option_disabled() -> Result<()> {
    let ui = session([Key::Down, Key::Enter]);
    let got = ui.ask(Option::<Plan>::prompt("Plan"))?;
    assert_eq!(got, None);
    Ok(())
}

#[derive(Debug, PartialEq, clacky::Choices)]
enum Step {
    Build,
    Skip,
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Pipeline {
    step: Option<Step>,
}

#[test]
fn the_skip_row_is_a_message_and_not_a_variant() -> Result<()> {
    let german = Messages::english().skip("Ueberspringen");
    let ui = session([Key::Down, Key::Down, Key::Enter]).messages(german);
    let got = ui.ask(Pipeline::form())?;
    assert_eq!(got, Pipeline { step: None });
    let shown = screen(&ui.terminal().written());
    assert!(shown.contains("Ueberspringen"), "{shown}");
    let some = session([]).ask(Pipeline::form().step(Some(Some(Step::Skip))))?;
    assert_eq!(
        some,
        Pipeline {
            step: Some(Step::Skip)
        }
    );
    let none = session([]).ask(Pipeline::form().step(Some(None)))?;
    assert_eq!(none, Pipeline { step: None });
    Ok(())
}

#[test]
fn a_variant_labelled_skip_does_not_share_a_name_with_the_skip_row() -> Result<()> {
    let ui = session([Key::Down, Key::Down, Key::Enter]);
    assert_eq!(ui.ask(Pipeline::form())?, Pipeline { step: None });
    let shown = screen(&ui.terminal().written());
    assert!(shown.contains("(Skip)"), "{shown}");
    let line = |text: &'static str| {
        let term = Script::new(TerminalKind::Dumb, [], [text]);
        Session::new(term, Settings::new(ColorChoice::Plain)).ask(Pipeline::form())
    };
    assert_eq!(
        line("skip")?,
        Pipeline {
            step: Some(Step::Skip)
        }
    );
    assert_eq!(line("(skip)")?, Pipeline { step: None });
    Ok(())
}

/// Names the user module may have in scope must not change the expansion.
mod hygiene {

    use super::*;

    #[derive(Debug, PartialEq, Clone, Copy)]
    enum Rung {
        None,
        Some,
    }

    use Rung::*;

    /// Uses the imported names, so the glob import is not unused.
    const RUNGS: [Rung; 2] = [None, Some];

    #[derive(Debug, PartialEq, clacky::Choices)]
    enum T {
        A,
        B,
    }

    #[derive(Debug, PartialEq, clacky::Form)]
    struct Mixed {
        t: T,
        #[ask(default = 8080)]
        port: u16,
        #[ask(skip)]
        rung: Option<u8>,
    }

    #[test]
    fn a_type_named_t_and_imported_none_do_not_break_the_expansion() -> Result<()> {
        assert_eq!(RUNGS.len(), 2);
        let got = defaulting().ask(Mixed::form())?;
        assert_eq!(
            got,
            Mixed {
                t: T::A,
                port: 8080,
                rung: Option::None
            }
        );
        Ok(())
    }
}

#[test]
fn an_optional_number_takes_only_what_a_number_takes() -> Result<()> {
    let nan = session(typed("nan").chain([Key::Enter]));
    assert_eq!(nan.ask(Option::<f64>::prompt("Ratio"))?, None);
    let big = session(typed("300").chain([Key::Enter]));
    assert!(big.ask(Option::<u8>::prompt("Retries")).is_err());
    let shown = big.terminal().written();
    assert!(
        shown.contains("Enter a number between 0 and 255"),
        "{shown}"
    );
    let some = session(typed("7").chain([Key::Enter]));
    assert_eq!(some.ask(Option::<u8>::prompt("Retries"))?, Some(7));
    Ok(())
}
