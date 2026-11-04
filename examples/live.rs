//! Live output: a spinner, a progress bar, tasks, a task log and a text stream.

use std::{thread::sleep, time::Duration};

use clacky::{Tone, output::spinner::DELAY, prelude::*};

fn pause(ticks: u32, mut tick: impl FnMut() -> Result<()>) -> Result<()> {
    for _ in 0..ticks {
        sleep(DELAY);
        tick()?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" live ")?;

    let mut spinner = ui.spinner();
    spinner.start("Resolving")?;
    pause(15, || spinner.tick())?;
    spinner.message("Resolving 2 of 2")?;
    pause(15, || spinner.tick())?;
    spinner.stop("Resolved")?;

    let mut bar = ui.progress(10);
    bar.start("Downloading")?;
    for step in 1..=10 {
        sleep(Duration::from_millis(60));
        bar.advance(step, format!("Downloading {step} of 10"))?;
    }
    bar.stop("Downloaded")?;

    ui.tasks()
        .task("Unpacking", |running| {
            pause(10, || running.tick())?;
            Ok(Some("Unpacked".to_owned()))
        })
        .task("Linking", |running| {
            pause(10, || running.tick())?;
            Ok(None)
        })
        .run()?;

    let mut log = ui.task_log("Building");
    for line in ["compiling a", "compiling b", "linking"] {
        sleep(Duration::from_millis(150));
        log.message(line)?;
    }
    log.success("Built")?;

    let mut stream = ui.stream(Tone::Info);
    for piece in ["Done ", "in ", "a ", "few ", "pieces."] {
        sleep(Duration::from_millis(120));
        stream.write(piece)?;
    }
    stream.finish()?;

    ui.outro("Done")
}
