# Testing a form

With the `fake` feature, [`fake::Script`](crate::fake::Script) is a scripted terminal: it hands out the keys you give it and records what was written. Write the form as a function over the session, and run it on a script.

```
use clacky::{
    core::{error::Result, settings::Settings},
    fake::Script,
    prelude::*,
    render::color::ColorChoice,
    terminal::{Terminal, TerminalKind},
};

fn setup(ui: &Session<impl Terminal>) -> Result<(String, bool)> {
    let name = ui.ask(text("Name").default("app"))?;
    let ok = ui.ask(confirm("Create it?"))?;
    Ok((name, ok))
}

fn main() -> Result<()> {
    let keys = "ada".chars().map(Key::Char).chain([Key::Enter, Key::Enter]);
    let ui = Session::new(
        Script::new(TerminalKind::Live, keys, []),
        Settings::new(ColorChoice::Plain),
    );
    assert_eq!(setup(&ui)?, ("ada".to_owned(), true));
    Ok(())
}
```

`written()`, `frames()` and [`fake::screen`](crate::fake::screen) give back what a person would have seen. `TerminalKind::Dumb` and `Redirected` scripts test the line paths, reading the lines of the third argument, and `Script::stopped_at_key(n)` stops a question at the nth read to test `Error::Aborted`.
