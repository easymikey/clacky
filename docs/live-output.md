# Live output

Output that is not a question hangs off the session too: `log`, `note` and
`boxed` print once, and `spinner`, `progress`, `tasks`, `stream` and `task_log`
stay open while work runs. Asking while a spinner runs is `Error::Busy`.

```no_run
use clacky::{Tone, prelude::*};

# fn main() -> Result<()> {
let ui = Session::detect()?;
ui.log(Tone::Success, "Installed")?;
ui.boxed("Saved", "Your settings are in `~/.config/acme`.").show()?;

// Your clock calls `tick`; the library starts no thread.
let mut spinner = ui.spinner();
spinner.start("Installing")?;
spinner.tick()?;
spinner.stop("Installed")?;

// Closures one after another, each under a spinner.
ui.tasks()
    .task("Fetching", |spinner| {
        spinner.message("Fetching 2 of 2")?;
        Ok(Some("Fetched".to_owned()))
    })
    .task("Linking", |_| Ok(None))
    .run()?;
# Ok(())
# }
```

With the `tokio` feature, `ui.async_tasks()`, `Spinner::during(future)` and
`ui.wait(label, future)` run futures under the same spinner.
