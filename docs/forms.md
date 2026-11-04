# Forms with derive

With the `derive` feature an enum becomes a `select` and a struct becomes a form asked field by field. The dependency must be named `clacky`:

```toml
clacky = { version = "0.1", features = ["derive"] }
```

```no_run
use clacky::prelude::*;

#[derive(Debug, Choices)]
enum Kind {
    #[ask("Library")]
    Lib,
    #[ask("Binary", hint = "recommended")]
    Bin,
}

#[derive(Debug, Form)]
struct Setup {
    #[ask("Project name", default = "my-app")]
    name: String,
    #[ask(default = 8080)]
    port: u16,
    kind: Kind,
}

fn main() -> Result<()> {
    let ui = Session::detect()?;
    let setup = ui.ask(Setup::form().port(Some(3000)))?;
    ui.outro(format_args!("{setup:?}"))
}
```

`Setup::form()` has a setter per field taking an `Option`, and only the fields left `None` are asked. The [`core::askable`](crate::core::askable) page lists the field types and options.
