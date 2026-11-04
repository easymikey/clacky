# Live output

Output that is not a question hangs off the session too. `log`, `note` and
`boxed` print once; `stream`, `spinner`, `progress`, `tasks` and `task_log`
stay open while work runs. Only one question or one spinner runs at a time,
and asking while a spinner runs is `Error::Busy`.

```no_run
use clacky::{BoxWidth, NoteLine, NoteTone, Tone, prelude::*};

# fn main() -> Result<()> {
let ui = Session::detect()?;
ui.log(Tone::Success, "Installed")?;
ui.note(
    "Next steps",
    &[NoteLine {
        tone: NoteTone::Plain,
        text: "cd my-app".into(),
    }],
)?;
ui.boxed("Saved", "Your settings are in `~/.config/acme`.")
    .width(BoxWidth::Fraction(1, 2))
    .show()?;

// Text that arrives in pieces.
let mut stream = ui.stream(Tone::Info);
stream.write("Hello, wor")?;
stream.write("ld")?;
stream.finish()?;

// Your clock calls `tick`; the library starts no thread.
let mut spinner = ui.spinner();
spinner.start("Installing")?;
spinner.tick()?;
spinner.stop("Installed")?;

let mut bar = ui.progress(3);
bar.start("Copying")?;
bar.advance(2, "Copying b")?;
bar.stop("Copied")?;

// Closures one after another, each under a spinner.
ui.tasks()
    .task("Fetching", |spinner| {
        spinner.message("Fetching 2 of 2")?;
        Ok(Some("Fetched".to_owned()))
    })
    .task("Linking", |_| Ok(None))
    .run()?;

// Shows the last lines while it runs.
let mut log = ui.task_log("Building");
log.message("compiling a")?;
log.success("Built")?;
# Ok(())
# }
```

With the `tokio` feature, `AsyncTasks` (`ui.async_tasks()`),
`Spinner::during(future)` and `ui.wait(label, future)` run futures under the
same spinner, and `ui.ask_until` asks again while an async check refuses.
`examples/login.rs` shows `ask_until`.
