//! `#[derive(Choices)]` and `#[derive(Form)]`, with answers that came from the
//! command line asked no more.

use clacky::prelude::*;

#[derive(Debug, Clone, Choices)]
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

#[derive(Default)]
struct Args {
    name: Option<String>,
    kind: Option<Kind>,
    yes: bool,
}

fn args() -> Args {
    let mut args = Args::default();
    let mut words = std::env::args().skip(1);
    while let Some(word) = words.next() {
        match word.as_str() {
            "--name" => args.name = words.next(),
            "--lib" => args.kind = Some(Kind::Lib),
            "--bin" => args.kind = Some(Kind::Bin),
            "--yes" => args.yes = true,
            _ => {}
        }
    }
    args
}

fn main() -> Result<()> {
    let args = args();
    let defaults = if args.yes {
        Defaults::Take
    } else {
        Defaults::Ask
    };
    let ui = Session::detect()?.defaults(defaults);

    ui.intro("derive")?;
    let setup = ui.ask(Setup::form().name(args.name).kind(args.kind))?;
    ui.outro(format_args!(
        "{} on port {}, {:?}",
        setup.name, setup.port, setup.kind
    ))
}
