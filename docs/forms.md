
# Forms with derive

With the `derive` feature an enum becomes a `select` over its variants and a
struct becomes a form asked field by field. Neither needs `PartialEq`. The
dependency must be named `clacky`, with the feature on:

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

#[derive(Debug, Choices)]
enum Tool {
    Fmt,
    Lint,
}

#[derive(Debug, Form)]
struct Setup {
    #[ask("Project name", default = "my-app")]
    name: String,
    #[ask(default = 8080)]
    port: u16,
    kind: Kind,
    #[ask(optional)]
    tools: Vec<Tool>,
}

fn main() -> Result<()> {
    let ui = Session::detect()?;
    let setup = ui.ask(Setup::form())?;
    ui.outro(&format!("{setup:?}"))
}
```

Field options are `default`, `placeholder`, `validate`, `optional`, `parse`
and `skip`. A field is a `String`, a number, `bool`, `Date`, `PathBuf`, an enum
with `#[derive(Choices)]`, a `Vec<E>` or `Option<E>` of such an enum, or an
`Option<String>` or `Option<number>`. Any other type that has `FromStr` and
`Display` takes `#[ask(parse)]`. The [`core::askable`](crate::core::askable)
page has the table, with what each field's `validate` check receives.

`Setup::form()` has a setter per field that takes an `Option`, for answers that
came from the command line. `ui.ask(Setup::form().name(args.name).port(args.port))`
asks only the fields left `None`. Under [`Defaults::Take`](crate::core::settings::Defaults)
an enum field takes its first variant, an `Option` field takes `None` and an
`optional` `Vec` takes an empty list. `examples/derive.rs` runs the whole thing.
