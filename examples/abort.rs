//! Ending a question from outside: a deadline and a thread that calls `stop`.

use std::{thread, time::Duration};

use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" abort ")?;

    match ui.ask(text("Name").timeout(Duration::from_secs(10))) {
        Err(Error::Aborted) => return ui.cancel("Too slow"),
        other => ui.log(clacky::Tone::Info, format_args!("Hello, {}", other?))?,
    }

    let (abort, stop) = Abort::new();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(8));
        stop.stop();
    });
    match ui.ask(confirm("Still there?").abort(&abort)) {
        Err(Error::Aborted) => ui.cancel("The watchdog ended the question"),
        answer => ui.outro(format_args!("Answered: {}", answer?)),
    }
}
