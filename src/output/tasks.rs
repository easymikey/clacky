//! `tasks`: titled steps run one after another, each with a spinner.

use std::{fmt::Display, pin::Pin};

use crate::{
    core::{error::Error, session::Session},
    output::spinner::Spinner,
    terminal::Terminal,
};

/// What a task gives back: the text its line ends with, or `None` for its
/// title; or the error that failed it.
pub type Outcome<E = Error> = Result<Option<String>, E>;

type Work<'s, T, E> =
    Box<dyn for<'a, 'b> FnOnce(&'a mut Spinner<'b, T>) -> Outcome<E> + 's>;

/// Closures run in order, each under a spinner. A failing task ends its line in
/// red and stops the run; the error is [`Error`] unless
/// [`Session::tasks_failing_with`] names one. Ctrl-C is seen at the next
/// `tick`, `message` or `stop` of a task's spinner; work that blocks holds it
/// until then.
///
/// ```no_run
/// # use clacky::{core::session::Session, terminal::Terminal};
/// # fn setup<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// session
///     .tasks()
///     .task("Fetching", |spinner| {
///         spinner.message("Fetching 2 of 2")?;
///         Ok(Some("Fetched".to_owned()))
///     })
///     .task("Linking", |_| Ok(None))
///     .run()?;
/// # Ok(()) }
/// ```
#[must_use]
pub struct Tasks<'s, T: Terminal, E = Error> {
    session: &'s Session<T>,
    items: Vec<(String, Work<'s, T, E>)>,
}

impl<T: Terminal, E> std::fmt::Debug for Tasks<'_, T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tasks")
            .field(
                "titles",
                &self
                    .items
                    .iter()
                    .map(|(title, _)| title)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl<T: Terminal> Session<T> {
    /// An empty list of tasks that fail with [`Error`].
    pub fn tasks(&self) -> Tasks<'_, T> {
        self.tasks_failing_with()
    }

    /// An empty list of tasks that fail with `E`, which the run gives back.
    pub fn tasks_failing_with<E: From<Error> + Display>(&self) -> Tasks<'_, T, E> {
        Tasks {
            session: self,
            items: Vec::new(),
        }
    }
}

impl<'s, T: Terminal, E: From<Error> + Display> Tasks<'s, T, E> {
    /// Adds a task under `title`.
    pub fn task(
        mut self,
        title: impl Into<String>,
        work: impl for<'a, 'b> FnOnce(&'a mut Spinner<'b, T>) -> Outcome<E> + 's,
    ) -> Self {
        self.items.push((title.into(), Box::new(work)));
        self
    }

    /// Runs the tasks in order.
    /// # Errors
    /// The terminal failed, a task failed, or Ctrl-C came; the rest do not run.
    pub fn run(self) -> Result<(), E> {
        self.items.into_iter().try_for_each(|(title, work)| {
            let mut spinner = self.session.spinner();
            spinner.start(&title)?;
            let outcome = work(&mut spinner);
            finish(&mut spinner, &title, outcome)
        })
    }
}

fn finish<T: Terminal, E: From<Error> + Display>(
    spinner: &mut Spinner<'_, T>,
    title: &str,
    outcome: Outcome<E>,
) -> Result<(), E> {
    match outcome {
        Ok(text) => spinner
            .stop(
                text.filter(|text| !text.is_empty())
                    .unwrap_or_else(|| title.to_owned()),
            )
            .map_err(E::from),
        // A spinner that Ctrl-C ended is finished already and ignores this.
        Err(error) => spinner
            .error(format!("{title}: {error}"))
            .map_err(E::from)
            .and(Err(error)),
    }
}

type Step<'s, E> = Pin<Box<dyn Future<Output = Outcome<E>> + Send + 's>>;

/// Futures run in order, each under a spinner that ticks on the clock.
///
/// ```no_run
/// # use clacky::{core::session::Session, terminal::Terminal};
/// # async fn setup<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// session
///     .async_tasks()
///     .task("Fetching", async { Ok(Some("Fetched".to_owned())) })
///     .task("Linking", async { Ok(None) })
///     .run()
///     .await?;
/// # Ok(()) }
/// ```
#[must_use]
pub struct AsyncTasks<'s, T: Terminal, E = Error> {
    session: &'s Session<T>,
    items: Vec<(String, Step<'s, E>)>,
}

impl<T: Terminal, E> std::fmt::Debug for AsyncTasks<'_, T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncTasks")
            .field(
                "titles",
                &self
                    .items
                    .iter()
                    .map(|(title, _)| title)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl<T: Terminal> Session<T> {
    /// An empty list of tasks that are futures and fail with [`Error`].
    pub fn async_tasks(&self) -> AsyncTasks<'_, T> {
        self.async_tasks_failing_with()
    }

    /// An empty list of tasks that are futures and fail with `E`, which the run
    /// gives back.
    pub fn async_tasks_failing_with<E: From<Error> + Display>(
        &self,
    ) -> AsyncTasks<'_, T, E> {
        AsyncTasks {
            session: self,
            items: Vec::new(),
        }
    }
}

impl<'s, T: Terminal, E: From<Error> + Display> AsyncTasks<'s, T, E> {
    /// Adds a task under `title`.
    pub fn task(
        mut self,
        title: impl Into<String>,
        work: impl Future<Output = Outcome<E>> + Send + 's,
    ) -> Self {
        self.items.push((title.into(), Box::pin(work)));
        self
    }

    /// Runs the tasks in order.
    /// # Errors
    /// The terminal failed, a task failed, or Ctrl-C came; the rest do not run.
    pub async fn run(self) -> Result<(), E> {
        for (title, work) in self.items {
            let mut spinner = self.session.spinner();
            spinner.start(&title)?;
            let outcome = spinner
                .during(work)
                .await
                .map_err(E::from)
                .and_then(|done| done);
            finish(&mut spinner, &title, outcome)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::panic_in_result_fn, reason = "assert is the point of a test")]

    use std::cell::RefCell;

    use crate::{
        core::{
            error::{Error, Result},
            session::Session,
            settings::{OutputMode, Settings},
        },
        fake::{Script, screen},
        render::color::ColorChoice,
        terminal::{Console, TerminalKind},
    };

    fn session(kind: TerminalKind) -> Session<Script> {
        Session::new(Script::new(kind, [], []), Settings::new(ColorChoice::Plain))
    }

    #[test]
    fn accessible_output_gives_each_task_plain_lines() -> Result<()> {
        let session = session(TerminalKind::Live).output(OutputMode::Accessible);
        session
            .tasks()
            .task("Fetching", |_| Ok(Some("Fetched".to_owned())))
            .task("Linking", |_| Ok(None))
            .run()?;
        let raw = session.terminal().written();
        assert!(!raw.contains('\x1b'), "{raw:?}");
        assert!(raw.contains("◇  Fetched\n"), "{raw:?}");
        assert!(raw.ends_with("◇  Linking\n"), "{raw:?}");
        Ok(())
    }

    #[test]
    fn each_task_gets_its_own_line_in_order() -> Result<()> {
        let session = session(TerminalKind::Live);
        let seen = RefCell::new(Vec::new());
        session
            .tasks()
            .task("Fetching", |spinner| {
                seen.borrow_mut().push("fetch");
                spinner.message("Fetching 2 of 2")?;
                Ok(Some("Fetched".to_owned()))
            })
            .task("Linking", |_| {
                seen.borrow_mut().push("link");
                Ok(None)
            })
            .run()?;
        assert_eq!(*seen.borrow(), ["fetch", "link"]);
        assert_eq!(
            screen(&session.terminal().written()),
            "│\n◇  Fetched\n│\n◇  Linking"
        );
        Ok(())
    }

    #[test]
    fn a_failed_task_ends_red_and_stops_the_run() {
        let session = session(TerminalKind::Live);
        let ran = RefCell::new(false);
        let done = session
            .tasks()
            .task("Fetching", |_| Err(Error::Cancelled))
            .task("Linking", |_| {
                *ran.borrow_mut() = true;
                Ok(None)
            })
            .run();
        assert!(matches!(done, Err(Error::Cancelled)), "{done:?}");
        assert!(!*ran.borrow());
        let shown = screen(&session.terminal().written());
        assert_eq!(shown, "│\n▲  Fetching: cancelled");
    }

    #[derive(Debug)]
    enum Failure {
        Download(&'static str),
        Terminal(Error),
    }

    impl std::fmt::Display for Failure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Download(why) => write!(f, "download failed: {why}"),
                Self::Terminal(error) => error.fmt(f),
            }
        }
    }

    impl From<Error> for Failure {
        fn from(error: Error) -> Self {
            Self::Terminal(error)
        }
    }

    #[test]
    fn a_task_fails_with_its_own_error() {
        let session = session(TerminalKind::Live);
        let done = session
            .tasks_failing_with::<Failure>()
            .task("Fetching", |spinner| {
                spinner.tick()?;
                Err(Failure::Download("connection refused"))
            })
            .run();
        assert!(
            matches!(done, Err(Failure::Download("connection refused"))),
            "{done:?}"
        );
        assert_eq!(
            screen(&session.terminal().written()),
            "│\n▲  Fetching: download failed: connection refused"
        );
    }

    #[test]
    fn an_empty_result_falls_back_to_the_title() -> Result<()> {
        let session = session(TerminalKind::Redirected);
        session
            .tasks()
            .task("Fetching", |_| Ok(Some(String::new())))
            .run()?;
        assert_eq!(
            session.terminal().written(),
            "│\n◒  Fetching...\n◇  Fetching\n"
        );
        Ok(())
    }

    #[test]
    fn the_async_run_can_be_sent_to_another_thread() {
        fn spawnable(
            session: &Session<Console>,
        ) -> impl Future<Output = Result<()>> + Send + '_ {
            session
                .async_tasks()
                .task("Fetching", async { Ok(None) })
                .run()
        }
        std::hint::black_box(spawnable);
    }

    #[tokio::test]
    async fn futures_run_in_order_under_a_spinner() -> Result<()> {
        let session = session(TerminalKind::Live);
        session
            .async_tasks()
            .task("Fetching", async { Ok(Some("Fetched".to_owned())) })
            .task("Linking", async { Ok(None) })
            .run()
            .await?;
        let raw = session.terminal().written();
        assert!(raw.contains("◒  Fetching"), "{raw:?}");
        assert_eq!(screen(&raw), "│\n◇  Fetched\n│\n◇  Linking");
        Ok(())
    }

    #[tokio::test]
    async fn a_failed_future_ends_red() {
        let session = session(TerminalKind::Live);
        let done = session
            .async_tasks()
            .task("Fetching", async { Err(Error::Cancelled) })
            .run()
            .await;
        assert!(matches!(done, Err(Error::Cancelled)), "{done:?}");
        assert_eq!(
            screen(&session.terminal().written()),
            "│\n▲  Fetching: cancelled"
        );
    }

    #[tokio::test]
    async fn a_future_fails_with_its_own_error() {
        let session = session(TerminalKind::Live);
        let done = session
            .async_tasks_failing_with::<Failure>()
            .task("Fetching", async { Err(Failure::Download("timed out")) })
            .run()
            .await;
        assert!(
            matches!(done, Err(Failure::Download("timed out"))),
            "{done:?}"
        );
        assert_eq!(
            screen(&session.terminal().written()),
            "│\n▲  Fetching: download failed: timed out"
        );
    }
}
