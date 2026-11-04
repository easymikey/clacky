
# Testing a form

With the `fake` feature, [`fake::Script`](crate::fake::Script) is a scripted
terminal: it hands out the keys and lines you give it and records what was
written. Write the form as a function over the session, and run it on a script.

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

`Script` also keeps what was written (`written()` and `frames()`), and
[`fake::screen`](crate::fake::screen) replays the escapes into the text a person
would see. Enable the feature in your dev-dependencies:

```toml
[dev-dependencies]
clacky = { version = "0.1", features = ["fake"] }
```

The same script can be a `Dumb` or `Redirected` terminal, which tests the paths
without a keyboard. The third argument is the lines those terminals read:

```
use clacky::{
    core::settings::Settings,
    fake::Script,
    prelude::*,
    render::color::ColorChoice,
    terminal::TerminalKind,
};

# fn main() -> Result<()> {
let ui = Session::new(
    Script::new(TerminalKind::Dumb, [], ["2"]),
    Settings::new(ColorChoice::Plain),
);
let kind = ui.ask(select("Kind").item(1, "Library").item(2, "Binary"))?;
assert_eq!(kind, 2);
# Ok(())
# }
```

A deadline ends a question where nobody types, and the answer is
`Error::Aborted`. `Script::stopped_at_key(n)` and `stopped_at_line(n)` call the
stop of the question at the nth read, as a watcher thread would.

```
use std::time::Duration;

use clacky::{
    core::{error::Error, settings::Settings},
    fake::Script,
    prelude::*,
    render::color::ColorChoice,
    terminal::TerminalKind,
};

let script = Script::new(TerminalKind::Live, [], []);
let ui = Session::new(script, Settings::new(ColorChoice::Plain));
let asked = ui.ask(text("Name").timeout(Duration::from_millis(20)));
assert!(matches!(asked, Err(Error::Aborted)));
```
