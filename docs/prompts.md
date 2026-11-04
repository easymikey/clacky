# Prompts

Every prompt is a free function that returns a builder; `ui.ask(prompt)` runs
it and gives back a typed answer.

```no_run
use clacky::prelude::*;

# fn main() -> Result<()> {
let ui = Session::detect()?;
let name = ui.ask(text("Name").placeholder("Ada"))?;
let ok = ui.ask(confirm("Sure?"))?;
let port = ui.ask(
    select("Port")
        .item(8080, "Development")
        .item(443, "Production"),
)?;
# drop((name, ok, port));
# Ok(())
# }
```

## Checking answers

`.validate(check)` returns `Ok(())` or `Err(message)`, and a refused answer
keeps the question open under the message. `.parse::<T>()` and `.map(f)` change
the answer, and a tuple of prompts is asked in order. `simulate` runs a prompt
on a list of keys without a terminal, so it also serves as a quick try:

```
use clacky::prelude::*;

# fn main() -> Result<()> {
let keys = "a@b".chars().map(Key::Char).chain([Key::Enter]);
let email = text("Email").validate(|address: &str| {
    if address.contains('@') {
        Ok(())
    } else {
        Err("Needs an @.")
    }
});
assert_eq!(email.simulate(keys)?, "a@b");
# Ok(())
# }
```

## Prefill and `--yes`

`.given(Option<T>)` skips a question whose answer came from the command line,
and `.given_line(Option<&str>)` reads text from the environment as a typed
line. `Defaults::Take` answers the rest with their defaults, and a question
without one is `Error::NoDefault`. A `confirm` has one built in, Yes, so
`--yes` answers Yes unless `.default(Reply::No)` says otherwise.

```no_run
use clacky::prelude::*;

# fn main() -> Result<()> {
let (port, yes) = (Some(8080_u16), false);
let defaults = if yes { Defaults::Take } else { Defaults::Ask };
let ui = Session::detect()?.defaults(defaults);

let port = ui.ask(text("Port").default("8080").parse::<u16>().given(port))?;
let token = ui.ask(
    password("Token").given_line(std::env::var("APP_TOKEN").ok().as_deref()),
)?;
ui.outro(format_args!("port {port}, {} bytes of token", token.len()))
# }
```
