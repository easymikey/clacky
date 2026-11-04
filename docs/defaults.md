# Prefill and defaults

A CLI that asks for its missing arguments would repeat `match arg { Some(v)
=> v, None => prompt }` for every question. `.given(Option<T>)` does that
for any prompt: `Some` prints the answered line and asks nothing, `None`
asks. `.given_line(Option<&str>)` takes text from the environment or a file
and reads it as a typed line, so `validate` and `parse` still apply and a
refusal is `Error::Refused`.

`Defaults::Take` answers every question with its default, for `--yes` or CI.
A question without a default is `Error::NoDefault` naming it, so a
destructive confirm is never answered by accident. The library reads the
environment only in `Session::detect`; read `CI` yourself if it should mean
`--yes`.

```no_run
use clacky::prelude::*;

# fn main() -> Result<()> {
let (name, port, yes) = (None::<String>, Some(8080_u16), false);
let defaults = if yes { Defaults::Take } else { Defaults::Ask };
let ui = Session::detect()?.defaults(defaults);

let name = ui.ask(text("Project name").default("my-app").given(name))?;
let port = ui.ask(text("Port").default("8080").parse::<u16>().given(port))?;
let token = ui.ask(
    password("Token").given_line(std::env::var("APP_TOKEN").ok().as_deref()),
)?;
ui.outro(&format!("{name}:{port} ({} bytes of token)", token.len()))
# }
```
