# clacky

Clack-style prompts for Rust: one question at a time on a rail, answers as typed values, and a form you can test without a terminal. It is a port of [@clack/prompts](https://github.com/bombshell-dev/clack) with its look and keys.

```text
┌   create-app
│
◇  Where should we create your project?
│  ./sparkling-solid
│
◆  Pick a project type
│  ● TypeScript
│  ○ JavaScript
└
```

## Why

clacky is inspired by [clack](https://github.com/bombshell-dev/clack) 🍦: the prettiest terminal prompts I know, and I wanted them in Rust.
[cliclack](https://github.com/fadeevab/cliclack) ports the look but keeps the theme in a global, so you can't change colors per session or test your prompts.
clacky keeps clack's look and makes everything a value: the prompts, the theme, the texts.
It pulls in 4 crates where cliclack pulls in 41.

## Features

- **Prompts**: `text`, `password`, `multiline`, `confirm`, `number`, `date`, `path`, `select`, `select_key`, `multiselect`, `group_multiselect` and `autocomplete`, each with a builder.
- **Derive forms**: `#[derive(Choices)]` makes an enum a `select`, and `#[derive(Form)]` makes a struct a form asked field by field.
- **Live output**: spinner, progress bar, tasks, task log, text streams, notes and boxes, and futures under a spinner with `tokio`.
- **Theming by value**: theme, symbols, messages and key map are values on the session, so other languages and ASCII terminals need no global switch.
- **Prefill and `--yes`**: `.given(arg)` skips a question that was answered on the command line, and `Defaults::Take` answers the rest with their defaults.
- **Line terminals and accessible mode**: `TERM=dumb`, piped input and screen-reader output each have their own path, and a question nobody can answer is an error, not a hang.
- **Abort and timeout**: `.timeout(duration)` and `Abort` end a question from a deadline or another thread, and the terminal is put back.
- **Scripted tests**: `Prompt::simulate` runs a prompt on a list of keys, and `fake::Script` is a scripted terminal for a whole form.
- **Small**: 4 dependencies without `tokio`, about 410 KiB for a three question form.

## Install

```sh
cargo add clacky
cargo add clacky --features derive      # #[derive(Choices, Form)]
cargo add clacky --dev --features fake  # the scripted terminal, in tests
```

`tokio` is a default feature (async tasks and `ask_until`); `--no-default-features` drops it. The minimum Rust version is 1.85.

## Quick start

```rust,no_run
use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" my-app ")?;

    let name = ui.ask(text("Project name").placeholder("my-app").default("my-app"))?;
    let kind = ui.ask(select("Kind").item("lib", "Library").item("bin", "Binary"))?;
    let port: u16 = ui.ask(number("Port").min(1).max(65535).default(8080))?;

    if ui.ask(confirm("Create it?").default(Reply::Yes))? {
        ui.outro(&format!("{name}: {kind} on {port}"))
    } else {
        ui.cancel("Nothing created", &[])
    }
}
```

`Session::detect` reads the environment once. Esc and Ctrl-C come back as errors. The [guide](https://docs.rs/clacky/latest/clacky/guide/index.html) has a page per topic: prompts, forms, prefill, live output, theming, line terminals, abort, testing, and a size comparison with other libraries.

## Examples

Run one with `cargo run --example <name>`; `derive` needs `--features derive`.

| Example | Shows |
|---|---|
| `create_app` | the clack demo flow: a form, a spinner, a note |
| `select` | select, multiselect, groups, key shortcuts, a disabled option |
| `search` | autocomplete, its multiple form and a custom match rule |
| `files_and_dates` | `path` and `date` |
| `numbers` | `number` ranges and steps, `validate`, `parse` |
| `live` | spinner, progress, tasks, task log and a text stream |
| `abort` | a timeout and an `Abort` stopped from a thread |
| `defaults` | `.given`, `.given_line` and `--yes` with `Defaults::Take` |
| `i18n` | a theme, ASCII symbols and German messages |
| `accessible` | `OutputMode::Accessible` |
| `derive` | `Choices` and `Form` with answers from the command line |
| `login` | a password asked again while a server refuses it |
| `size` | the three question form that `just size` measures |

## Contributing

`just` runs every check.

## License

MIT or Apache-2.0, at your option.
