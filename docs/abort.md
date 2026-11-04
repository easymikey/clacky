# Cancel and abort

Esc (and Ctrl-D on an empty line) is `Error::Cancelled`; Ctrl-C is
`Error::Interrupted`. `Error::is_cancel` is true for both, and
`.cancel_as_none()` turns Esc into a value and leaves Ctrl-C an error.
Ctrl-C is caught only while a spinner, progress, tasks or a task log runs
(and during `Session::wait`): the output ends with its red cancel line and
the call returns `Error::Interrupted`. Everywhere else Ctrl-C works as the
program made it.

```no_run
use clacky::prelude::*;

# fn main() -> Result<()> {
let ui = Session::detect()?;
match ui.ask(confirm("Overwrite?")).cancel_as_none()? {
    Some(true) => ui.outro("Overwritten"),
    Some(false) | None => ui.cancel("Left as it was", &[]),
}
# }
```

Another thread, a timer or a signal watcher can end a question that waits
for the person. `Abort::new()` gives an `Abort` and the `Stop` that ends it
(`Stop` is `Send + Sync`). `.abort(&abort)` ties a question to it,
`.timeout(duration)` to a deadline that starts when the question is asked,
and `Session::abort(abort)` every question of the session. Whichever ends
first wins, and the answer is `Error::Aborted`, which is not a cancel. On a
terminal the cancelled frame stays and the terminal is put back; a dumb or
piped input is woken the same way, and what was typed of a line is not
taken. An abort that ended before the question asks it ends it at once and
draws nothing, also for a prefilled answer.

```no_run
use std::{thread, time::Duration};

use clacky::prelude::*;

# fn main() -> Result<()> {
let ui = Session::detect()?;
let (abort, stop) = Abort::new();
thread::spawn(move || {
    thread::sleep(Duration::from_secs(30));
    stop.stop();
});
let asked = ui.ask(text("Name").abort(&abort).timeout(Duration::from_secs(10)));
if matches!(asked, Err(Error::Aborted)) {
    return ui.cancel("Too slow", &[]);
}
# Ok(())
# }
```
