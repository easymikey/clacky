//! Signal handling while the terminal is altered.
use std::sync::{
    Mutex,
    PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::render::draw::{HIDE_CURSOR, SHOW_CURSOR};

static INTERRUPTS: AtomicU64 = AtomicU64::new(0);

/// Set when the last write hid the cursor; a signal handler shows it again.
static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);

pub(crate) fn note_cursor(text: &str) {
    let hides = text.rfind(HIDE_CURSOR);
    let shows = text.rfind(SHOW_CURSOR);
    if hides.is_some() || shows.is_some() {
        CURSOR_HIDDEN.store(hides > shows, Ordering::SeqCst);
    }
}

static CATCH: Mutex<Catch> = Mutex::new(Catch {
    holders: 0,
    saved: None,
});

struct Catch {
    holders: usize,
    saved: Option<imp::Saved>,
}

/// How many Ctrl-C signals were caught since the program started.
#[must_use]
pub fn interrupts() -> u64 {
    INTERRUPTS.load(Ordering::SeqCst)
}

#[must_use = "Ctrl-C is caught only while the guard lives"]
#[derive(Debug)]
/// Catches and counts Ctrl-C while it lives; dropping the last one restores the
/// previous handler.
pub struct Catching {
    holds: bool,
}

impl Catching {
    /// Starts catching Ctrl-C.
    pub fn start() -> Self {
        let mut catch = CATCH.lock().unwrap_or_else(PoisonError::into_inner);
        if catch.holders == 0 {
            catch.saved = imp::install();
        }
        catch.holders = catch.holders.saturating_add(1);
        Self { holds: true }
    }

    /// A guard that catches nothing, for terminals that have no signals.
    pub const fn nothing() -> Self {
        Self { holds: false }
    }
}

// GUARD: puts back the handler that `start` replaced, with the last holder.
impl Drop for Catching {
    fn drop(&mut self) {
        if !self.holds {
            return;
        }
        let mut catch = CATCH.lock().unwrap_or_else(PoisonError::into_inner);
        catch.holders = catch.holders.saturating_sub(1);
        if catch.holders == 0 {
            catch.saved.take().into_iter().for_each(imp::restore);
        }
    }
}

#[cfg(windows)]
#[must_use = "SIGTERM, SIGHUP and SIGINT are handled only while the guard lives"]
#[derive(Debug)]
/// Windows restores the console in its close handler instead.
pub(crate) struct Terminating;

#[cfg(windows)]
impl Terminating {
    pub(crate) const fn nothing() -> Self {
        Self
    }
}

#[cfg(unix)] pub(crate) use terminating::Terminating;

#[cfg(unix)]
mod terminating {
    use std::{
        mem,
        ptr,
        sync::{Mutex, PoisonError, atomic::Ordering},
    };

    use crate::render::draw::SHOW_CURSOR;

    const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGHUP, libc::SIGINT];

    struct State {
        holders: usize,
        /// The settings the first holder found, to put back.
        settings: Option<libc::termios>,
        /// The previous action of each signal in `SIGNALS` that was taken over.
        saved: [Option<libc::sigaction>; 3],
    }

    static STATE: Mutex<State> = Mutex::new(State {
        holders: 0,
        settings: None,
        saved: [None, None, None],
    });

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Step {
        PutBack,
        ShowCursor,
        Raise(libc::c_int),
    }

    pub(super) fn terminate(
        signal: libc::c_int,
        quieted: bool,
        cursor_hidden: bool,
        mut act: impl FnMut(Step),
    ) {
        if quieted {
            act(Step::PutBack);
        }
        if cursor_hidden {
            act(Step::ShowCursor);
        }
        act(Step::Raise(signal));
    }

    #[must_use = "SIGTERM, SIGHUP and SIGINT are handled only while the guard lives"]
    #[derive(Debug)]
    /// While this lives, SIGTERM, SIGHUP and SIGINT put the terminal back and
    /// show the cursor before ending the program.
    pub(crate) struct Terminating;

    impl Terminating {
        pub(crate) fn start(settings: libc::termios) -> Self {
            let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
            if state.holders == 0 {
                state.settings = Some(settings);
                state.saved = SIGNALS.map(replace);
            }
            state.holders = state.holders.saturating_add(1);
            Self
        }
    }

    // GUARD: puts back the handlers that `start` replaced, with the last
    // holder.
    impl Drop for Terminating {
        fn drop(&mut self) {
            let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
            state.holders = state.holders.saturating_sub(1);
            if state.holders == 0 {
                state.settings = None;
                let saved = mem::take(&mut state.saved);
                SIGNALS.into_iter().zip(saved).for_each(put_back);
            }
        }
    }

    /// Runs as the signal handler; it skips the restore if a guard starts or
    /// drops at that moment.
    extern "C" fn on_terminate(signal: libc::c_int) {
        let settings = STATE.try_lock().ok().and_then(|state| state.settings);
        let cursor_hidden = super::CURSOR_HIDDEN.load(Ordering::SeqCst);
        terminate(signal, settings.is_some(), cursor_hidden, |step| {
            match step {
                Step::PutBack => settings.iter().for_each(|settings| {
                    // SAFETY: `tcsetattr` only reads the `termios`; failure is
                    // ignored.
                    unsafe {
                        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, settings);
                    }
                }),
                Step::ShowCursor => {
                    // SAFETY: `write` reads the bytes of a constant; failure is
                    // ignored.
                    unsafe {
                        libc::write(
                            libc::STDERR_FILENO,
                            SHOW_CURSOR.as_ptr().cast(),
                            SHOW_CURSOR.len(),
                        );
                    }
                }
                // The action was reset on entry, so this ends the program.
                Step::Raise(again) => {
                    // SAFETY: `raise` takes a signal number; failure is
                    // ignored.
                    unsafe { libc::raise(again) };
                }
            }
        });
    }

    #[expect(
        clippy::as_conversions,
        reason = "`sigaction` stores a handler as an integer"
    )]
    /// Takes over `signal` only when it has its default action, which would
    /// kill the program with the terminal still quiet.
    fn replace(signal: libc::c_int) -> Option<libc::sigaction> {
        // SAFETY: an all-zero `sigaction` is valid, `sigaction` fills it in.
        let mut saved: libc::sigaction = unsafe { mem::zeroed() };
        // SAFETY: a null new action only reads the current one into the local.
        let read = unsafe { libc::sigaction(signal, ptr::null(), &raw mut saved) };
        if read != 0 || saved.sa_sigaction != libc::SIG_DFL {
            return None;
        }
        // SAFETY: an all-zero `sigaction` is valid: no flags, an empty mask.
        let mut action: libc::sigaction = unsafe { mem::zeroed() };
        let handler: extern "C" fn(libc::c_int) = on_terminate;
        action.sa_sigaction = handler as usize;
        // RESETHAND: the default action is back on entry; NODEFER: the signal
        // raised again is not held until the handler returns.
        action.sa_flags = libc::SA_RESETHAND | libc::SA_NODEFER;
        // SAFETY: both pointers lead to live locals; failure is ignored.
        unsafe { libc::sigaction(signal, ptr::from_ref(&action), ptr::null_mut()) };
        Some(saved)
    }

    /// Puts back the action `replace` saved, unless something took the signal
    /// over since (a spinner catching Ctrl-C).
    #[expect(
        clippy::as_conversions,
        reason = "`sigaction` stores a handler as an integer"
    )]
    fn put_back((signal, saved): (libc::c_int, Option<libc::sigaction>)) {
        // SAFETY: an all-zero `sigaction` is valid, `sigaction` fills it in.
        let mut current: libc::sigaction = unsafe { mem::zeroed() };
        // SAFETY: a null new action only reads the current one into the local.
        unsafe { libc::sigaction(signal, ptr::null(), &raw mut current) };
        let handler: extern "C" fn(libc::c_int) = on_terminate;
        if let Some(saved) = saved.filter(|_| current.sa_sigaction == handler as usize)
        {
            // SAFETY: the pointer leads to a live local; failure is ignored.
            unsafe { libc::sigaction(signal, ptr::from_ref(&saved), ptr::null_mut()) };
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::{mem, ptr, sync::atomic::Ordering};

    pub(super) type Saved = libc::sigaction;

    /// Runs as the signal handler: touches an atomic only, signal-safe.
    pub(super) extern "C" fn on_interrupt(_: libc::c_int) {
        super::INTERRUPTS.fetch_add(1, Ordering::SeqCst);
    }

    #[expect(
        clippy::as_conversions,
        reason = "`sigaction` stores a handler as an integer"
    )]
    pub(super) fn install() -> Option<Saved> {
        // SAFETY: an all-zero `sigaction` is valid: no flags, an empty mask.
        let mut action: libc::sigaction = unsafe { mem::zeroed() };
        let handler: extern "C" fn(libc::c_int) = on_interrupt;
        action.sa_sigaction = handler as usize;
        // Reads and writes elsewhere in the program go on after the signal.
        action.sa_flags = libc::SA_RESTART;
        // SAFETY: an all-zero `sigaction` is valid, `sigaction` fills it in.
        let mut saved: libc::sigaction = unsafe { mem::zeroed() };
        // SAFETY: both pointers lead to live locals.
        let status = unsafe {
            libc::sigaction(libc::SIGINT, ptr::from_ref(&action), &raw mut saved)
        };
        (status == 0).then_some(saved)
    }

    pub(super) fn restore(saved: Saved) {
        // SAFETY: the pointer leads to a live local; failure is ignored.
        unsafe {
            libc::sigaction(libc::SIGINT, ptr::from_ref(&saved), ptr::null_mut())
        };
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::Ordering;

    use windows_sys::{
        Win32::System::Console::{CTRL_C_EVENT, SetConsoleCtrlHandler},
        core::BOOL,
    };

    pub(super) type Saved = ();

    /// Runs on a thread of its own; 1 means the event is handled here.
    unsafe extern "system" fn on_event(event: u32) -> BOOL {
        if event == CTRL_C_EVENT {
            super::INTERRUPTS.fetch_add(1, Ordering::SeqCst);
            1
        } else {
            0
        }
    }

    pub(super) fn install() -> Option<Saved> {
        // SAFETY: the handler only touches an atomic; failure is ignored.
        (unsafe { SetConsoleCtrlHandler(Some(on_event), 1) } != 0).then_some(())
    }

    pub(super) fn restore((): Saved) {
        // SAFETY: removes the handler `install` added; failure is ignored.
        unsafe { SetConsoleCtrlHandler(Some(on_event), 0) };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        mem,
        ptr,
        sync::{Mutex, PoisonError, atomic::Ordering},
    };

    use crate::{
        render::draw::{HIDE_CURSOR, SHOW_CURSOR},
        terminal::signals::{
            CURSOR_HIDDEN,
            Catching,
            Terminating,
            imp,
            interrupts,
            note_cursor,
            terminating::{Step, terminate},
        },
    };

    fn handler_of(signal: libc::c_int) -> usize {
        // SAFETY: an all-zero `sigaction` is valid, `sigaction` fills it in.
        let mut current: libc::sigaction = unsafe { mem::zeroed() };
        // SAFETY: a null new action only reads the current one into the local.
        unsafe { libc::sigaction(signal, ptr::null(), &raw mut current) };
        current.sa_sigaction
    }

    /// Tests that change what SIGTERM, SIGHUP and SIGINT do run one at a time.
    static TERMINATION: Mutex<()> = Mutex::new(());

    fn set_handler(signal: libc::c_int, handler: usize) -> libc::sigaction {
        // SAFETY: an all-zero `sigaction` is valid: no flags, an empty mask.
        let mut action: libc::sigaction = unsafe { mem::zeroed() };
        action.sa_sigaction = handler;
        // SAFETY: an all-zero `sigaction` is valid, `sigaction` fills it in.
        let mut before: libc::sigaction = unsafe { mem::zeroed() };
        // SAFETY: both pointers lead to live locals.
        unsafe { libc::sigaction(signal, &raw const action, &raw mut before) };
        before
    }

    fn handler_of_interrupt() -> usize {
        handler_of(libc::SIGINT)
    }

    fn handlers_of_termination() -> [usize; 3] {
        [libc::SIGTERM, libc::SIGHUP, libc::SIGINT].map(handler_of)
    }

    #[test]
    fn the_handler_is_replaced_only_while_something_catches() {
        let _alone = TERMINATION.lock().unwrap_or_else(PoisonError::into_inner);
        let before = handler_of_interrupt();
        let first = Catching::start();
        let second = Catching::start();
        let during = handler_of_interrupt();
        drop(first);
        let while_held = handler_of_interrupt();
        drop(second);
        assert_eq!(
            (during != before, while_held, handler_of_interrupt()),
            (true, during, before)
        );
    }

    #[test]
    fn a_caught_signal_is_counted() {
        let before = interrupts();
        imp::on_interrupt(libc::SIGINT);
        assert!(interrupts() > before);
    }

    #[test]
    #[expect(
        clippy::as_conversions,
        reason = "`sigaction` stores a handler as an integer"
    )]
    fn a_signal_the_program_ignores_or_handles_is_left_alone() {
        extern "C" fn own(_: libc::c_int) {}
        let _alone = TERMINATION.lock().unwrap_or_else(PoisonError::into_inner);
        let own_handler: extern "C" fn(libc::c_int) = own;
        let own_handler = own_handler as usize;
        let ignored = libc::SIG_IGN;
        let before_term = set_handler(libc::SIGTERM, ignored);
        let before_hup = set_handler(libc::SIGHUP, own_handler);
        let before_int = set_handler(libc::SIGINT, own_handler);
        // SAFETY: an all-zero `termios` is a valid value to hold.
        let settings: libc::termios = unsafe { mem::zeroed() };
        let guard = Terminating::start(settings);
        let during = handlers_of_termination();
        drop(guard);
        let after = handlers_of_termination();
        set_handler(libc::SIGTERM, before_term.sa_sigaction);
        set_handler(libc::SIGHUP, before_hup.sa_sigaction);
        set_handler(libc::SIGINT, before_int.sa_sigaction);
        assert_eq!(
            (during, after),
            (
                [ignored, own_handler, own_handler],
                [ignored, own_handler, own_handler]
            )
        );
    }

    #[test]
    fn the_termination_handlers_are_replaced_only_while_a_guard_lives() {
        let _alone = TERMINATION.lock().unwrap_or_else(PoisonError::into_inner);
        // SAFETY: an all-zero `termios` is a valid value to hold.
        let settings: libc::termios = unsafe { mem::zeroed() };
        let before = handlers_of_termination();
        let first = Terminating::start(settings);
        let second = Terminating::start(settings);
        let during = handlers_of_termination();
        drop(first);
        let while_held = handlers_of_termination();
        drop(second);
        assert_eq!(
            (during != before, while_held, handlers_of_termination()),
            (true, during, before)
        );
    }

    #[test]
    fn a_guard_takes_over_ctrl_c_only_while_it_lives() {
        let _alone = TERMINATION.lock().unwrap_or_else(PoisonError::into_inner);
        // SAFETY: an all-zero `termios` is a valid value to hold.
        let settings: libc::termios = unsafe { mem::zeroed() };
        let before = handler_of_interrupt();
        let guard = Terminating::start(settings);
        let during = handler_of_interrupt();
        drop(guard);
        assert_eq!((during != before, handler_of_interrupt()), (true, before));
    }

    #[test]
    #[expect(
        clippy::as_conversions,
        reason = "`sigaction` stores a handler as an integer"
    )]
    fn a_guard_leaves_a_ctrl_c_handler_set_after_it_started() {
        extern "C" fn own(_: libc::c_int) {}
        let _alone = TERMINATION.lock().unwrap_or_else(PoisonError::into_inner);
        let own_handler: extern "C" fn(libc::c_int) = own;
        let own_handler = own_handler as usize;
        // SAFETY: an all-zero `termios` is a valid value to hold.
        let settings: libc::termios = unsafe { mem::zeroed() };
        let before = handler_of_interrupt();
        let guard = Terminating::start(settings);
        set_handler(libc::SIGINT, own_handler);
        drop(guard);
        let after = handler_of_interrupt();
        set_handler(libc::SIGINT, before);
        assert_eq!(after, own_handler);
    }

    #[test]
    fn a_termination_puts_back_and_shows_before_it_raises() {
        let steps = |quieted, hidden| {
            let mut steps = Vec::new();
            terminate(libc::SIGTERM, quieted, hidden, |step| steps.push(step));
            steps
        };
        assert_eq!(
            steps(true, true),
            [Step::PutBack, Step::ShowCursor, Step::Raise(libc::SIGTERM)]
        );
        assert_eq!(
            steps(true, false),
            [Step::PutBack, Step::Raise(libc::SIGTERM)]
        );
        assert_eq!(steps(false, false), [Step::Raise(libc::SIGTERM)]);
    }

    #[test]
    fn the_last_cursor_escape_written_tells_whether_it_is_hidden() {
        let hidden_after = |text: &str| {
            note_cursor(text);
            CURSOR_HIDDEN.load(Ordering::SeqCst)
        };
        assert!(hidden_after(&format!("{HIDE_CURSOR}│\n")));
        assert!(hidden_after("no escape"));
        assert!(!hidden_after(&format!("{HIDE_CURSOR}{SHOW_CURSOR}")));
        assert!(hidden_after(&format!("{SHOW_CURSOR}{HIDE_CURSOR}")));
        assert!(!hidden_after(SHOW_CURSOR));
    }
}
