
```
use clacky::{
    core::{ask::Key, settings::Settings},
    fake::Script,
    prelude::Session,
    render::color::ColorChoice,
    terminal::TerminalKind,
};

#[derive(Debug, PartialEq, clacky::Choices)]
enum Kind {
    Lib,
    Bin,
}

#[derive(Debug, PartialEq, clacky::Choices)]
enum Tool {
    Fmt,
    Lint,
    Docs,
}

#[derive(Debug, PartialEq, clacky::Choices)]
enum License {
    Mit,
    Apache,
}

fn port_is_free(port: &u16) -> Result<(), String> {
    if *port < 1024 {
        Err("ports below 1024 need root".to_owned())
    } else {
        Ok(())
    }
}

#[derive(Debug, PartialEq, clacky::Form)]
struct Setup {
    #[ask("Project name", placeholder = "my-app")]
    name: String,
    #[ask("Port", default = 8080, validate = port_is_free)]
    port: u16,
    kind: Kind,
    #[ask("Tools", optional)]
    tools: Vec<Tool>,
    #[ask("Install now?", default = false)]
    install: bool,
    license: Option<License>,
}

# fn main() -> clacky::core::error::Result<()> {
let keys = [
    Key::Char('a'),
    Key::Enter, // name
    Key::Enter, // port: the default
    Key::Down,
    Key::Enter, // kind: Bin
    Key::Char(' '),
    Key::Enter,     // tools: Fmt
    Key::Char('y'), // install
    Key::Down,
    Key::Down,
    Key::Enter, // license: Skip
];
let ui = Session::new(
    Script::new(TerminalKind::Live, keys, []),
    Settings::new(ColorChoice::Plain),
);
assert_eq!(
    ui.ask(Setup::form())?,
    Setup {
        name: "a".to_owned(),
        port: 8080,
        kind: Kind::Bin,
        tools: vec![Tool::Fmt],
        install: true,
        license: None,
    },
);
# Ok(())
# }
```

A struct of unsupported field types, a tuple struct, a non-unit variant and options in the wrong place fail to compile:

```compile_fail
#[derive(clacky::Choices)]
enum Kind { Lib(u8) }
```

```compile_fail
#[derive(clacky::Form)]
struct Setup(u8);
```

```compile_fail
#[derive(clacky::Choices)]
enum Kind { #[ask("Lib", color = "red")] Lib }
```

```compile_fail
#[derive(clacky::Form)]
struct Setup { #[ask(placeholder = "yes")] install: bool }
```

```compile_fail
#[derive(clacky::Form)]
struct Setup { #[ask(optional)] name: String }
```
