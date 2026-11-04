//! Answers that came from outside: `.given`, `.given_line` and `--yes` with
//! `Defaults::Take`.

use clacky::prelude::*;

fn main() -> Result<()> {
    let mut words = std::env::args().skip(1);
    let mut name = None;
    let mut yes = false;
    while let Some(word) = words.next() {
        match word.as_str() {
            "--name" => name = words.next(),
            "--yes" => yes = true,
            _ => {}
        }
    }

    let defaults = if yes { Defaults::Take } else { Defaults::Ask };
    let ui = Session::detect()?.defaults(defaults);
    ui.intro(" defaults ")?;

    // `Some` prints the answered line and asks nothing; `None` asks, or takes
    // the default under --yes.
    let name = ui.ask(text("Project name").default("my-app").given(name))?;
    // No default: under --yes this is `Error::NoDefault` unless APP_TOKEN is
    // set.
    let token = ui.ask(
        password("Token").given_line(std::env::var("APP_TOKEN").ok().as_deref()),
    )?;
    let create = ui.ask(confirm("Create it?").default(Reply::Yes))?;

    ui.outro(&format!(
        "{name}: {} bytes of token, create: {create}",
        token.len()
    ))
}
