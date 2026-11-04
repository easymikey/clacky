//! Quieting stdin's terminal and putting its settings back.

use std::{
    io::{self, Write as _},
    sync::{Mutex, PoisonError},
};

use crate::terminal::signals;

static TERMINAL: Mutex<()> = Mutex::new(());

pub(crate) fn exclusive<R>(work: impl FnOnce() -> R) -> R {
    let _held = TERMINAL.lock().unwrap_or_else(PoisonError::into_inner);
    work()
}

/// Has Windows put the console mode back when the process is broken off.
#[cfg(windows)]
pub(crate) fn remember() {
    imp::remember();
}

pub(crate) fn discard_typeahead() {
    imp::discard_typeahead();
}

#[cfg(unix)] pub(crate) use imp::{columns, next_byte, next_line};
#[cfg(windows)] pub(crate) use imp::{handle, mode_of, set_mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// What to turn off on the terminal; a later variant turns off more.
pub(crate) enum TerminalQuiet {
    Echo,
    /// The echo and line editing, so keys arrive one by one.
    Keys,
}

/// How many guards of each kind live. The terminal is as quiet as the quietest
/// of them, whatever order they end in.
#[derive(Debug, Clone, Copy, Default)]
struct Holders {
    echo: usize,
    keys: usize,
}

impl Holders {
    const fn count(&mut self, quiet: TerminalQuiet) -> &mut usize {
        match quiet {
            TerminalQuiet::Echo => &mut self.echo,
            TerminalQuiet::Keys => &mut self.keys,
        }
    }

    const fn level(self) -> Option<TerminalQuiet> {
        if self.keys > 0 {
            Some(TerminalQuiet::Keys)
        } else if self.echo > 0 {
            Some(TerminalQuiet::Echo)
        } else {
            None
        }
    }

    fn enter(&mut self, quiet: TerminalQuiet) -> TerminalQuiet {
        let count = self.count(quiet);
        *count = count.saturating_add(1);
        self.level().unwrap_or(quiet)
    }

    fn leave(&mut self, quiet: TerminalQuiet) -> Option<TerminalQuiet> {
        let count = self.count(quiet);
        *count = count.saturating_sub(1);
        self.level()
    }
}

struct Held {
    holders: Holders,
    /// What the first guard found, to put back.
    original: Option<imp::TerminalSettings>,
}

static HELD: Mutex<Held> = Mutex::new(Held {
    holders: Holders { echo: 0, keys: 0 },
    original: None,
});

#[must_use = "the terminal is back to normal as soon as the guard is dropped"]
/// A quieted terminal. Dropping the last guard puts the settings back.
pub struct QuietedTerminal {
    quiet: TerminalQuiet,
    _terminating: signals::Terminating,
}

impl std::fmt::Debug for QuietedTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Quieted")
    }
}

pub(crate) fn quiet(quiet: TerminalQuiet) -> io::Result<QuietedTerminal> {
    exclusive(|| {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        let original = match held.original {
            Some(original) => original,
            None => imp::current()?,
        };
        let mut holders = held.holders;
        imp::apply(original, Some(holders.enter(quiet)))?;
        held.holders = holders;
        held.original = Some(original);
        Ok(original)
    })
    .map(|original| QuietedTerminal {
        quiet,
        _terminating: terminating(original),
    })
}

#[cfg(unix)]
fn terminating(settings: imp::TerminalSettings) -> signals::Terminating {
    signals::Terminating::start(settings)
}

#[cfg(windows)]
fn terminating(_: imp::TerminalSettings) -> signals::Terminating {
    signals::Terminating::nothing()
}

// GUARD: puts the terminal settings back that `quiet` changed, with the last
// guard.
impl Drop for QuietedTerminal {
    fn drop(&mut self) {
        // Deliver what was written first: a later Ctrl-C would flush it.
        imp::flush_output();
        warn(exclusive(|| leave(self.quiet)).as_deref());
    }
}

fn leave(quiet: TerminalQuiet) -> Option<String> {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let level = held.holders.leave(quiet);
    let original = if level.is_none() {
        held.original.take()
    } else {
        held.original
    };
    original.and_then(|original| {
        imp::apply(original, level).err().map(|error| {
            format!("warning: the terminal settings were not put back: {error}")
        })
    })
}

pub(crate) fn warn(warning: Option<&str>) {
    warning.map(stderr_line).unwrap_or_default();
}

pub(crate) fn stderr_line(text: &str) {
    writeln!(io::stderr().lock(), "{text}").unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use super::{
        Holders,
        TerminalQuiet::{Echo, Keys},
    };

    #[test]
    fn guards_dropped_in_start_order_put_back_with_the_last() {
        let mut holders = Holders::default();
        assert_eq!(holders.enter(Echo), Echo);
        assert_eq!(holders.enter(Echo), Echo);
        assert_eq!(holders.leave(Echo), Some(Echo));
        assert_eq!(holders.leave(Echo), None);
    }

    #[test]
    fn keys_end_quieter_than_the_echo_around_them() {
        let mut holders = Holders::default();
        assert_eq!(holders.enter(Echo), Echo);
        assert_eq!(holders.enter(Keys), Keys);
        assert_eq!(holders.enter(Echo), Keys);
        assert_eq!(holders.leave(Echo), Some(Keys));
        assert_eq!(holders.leave(Keys), Some(Echo));
        assert_eq!(holders.leave(Echo), None);
    }
}

#[cfg(unix)]
mod imp {
    use std::{
        io,
        iter,
        mem::MaybeUninit,
        os::fd::RawFd,
        ptr,
        sync::{Mutex, PoisonError},
        time::{Duration, Instant},
    };

    use zeroize::{Zeroize as _, Zeroizing};

    use crate::terminal::{LineRead, abort::Abort, tty::TerminalQuiet};

    /// What was read from stdin past the last line handed out. A stop that
    /// arrives while a line is half typed leaves the half here for the next
    /// read.
    static PENDING: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    pub(super) type TerminalSettings = libc::termios;

    pub(super) fn current() -> io::Result<TerminalSettings> {
        let mut out = MaybeUninit::<TerminalSettings>::zeroed();
        // SAFETY: `tcgetattr` only writes a `termios` into a local.
        let status = unsafe { libc::tcgetattr(libc::STDIN_FILENO, out.as_mut_ptr()) };
        (status == 0)
            // SAFETY: the call succeeded, so the value is initialised.
            .then(|| unsafe { out.assume_init() })
            .ok_or_else(io::Error::last_os_error)
    }

    fn write_settings(settings: &TerminalSettings) -> io::Result<()> {
        // SAFETY: `tcsetattr` only reads the `termios` behind the reference.
        let status =
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, settings) };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn flush_output() {
        // SAFETY: `tcdrain` takes a descriptor; failure is ignored.
        unsafe { libc::tcdrain(libc::STDERR_FILENO) };
    }

    pub(super) fn apply(
        original: TerminalSettings,
        level: Option<TerminalQuiet>,
    ) -> io::Result<()> {
        let quieted = match level {
            None => original,
            // NOFLSH: a Ctrl-C must not flush what the form already wrote.
            Some(TerminalQuiet::Echo) => TerminalSettings {
                c_lflag: (original.c_lflag & !libc::ECHO) | libc::NOFLSH,
                ..original
            },
            Some(TerminalQuiet::Keys) => {
                let mut raw = original;
                // SAFETY: `cfmakeraw` only edits the `termios` behind the
                // reference.
                unsafe { libc::cfmakeraw(ptr::from_mut(&mut raw)) };
                // Output keeps its translation: the form draws with `\n`.
                raw.c_oflag = original.c_oflag;
                raw
            }
        };
        write_settings(&quieted)
    }

    pub(super) fn discard_typeahead() {
        // SAFETY: `tcflush` takes a descriptor; failure is ignored.
        unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) };
    }

    /// The next byte of stdin. `None` when none came within `wait`, or, with
    /// no `wait`, when `abort` ended.
    pub(crate) fn next_byte(
        wait: Option<Duration>,
        abort: &Abort,
    ) -> io::Result<Option<u8>> {
        byte_from(libc::STDIN_FILENO, wait, abort)
    }

    /// The next line of stdin. Waits as [`next_byte`] does; what `abort` cut
    /// short of a line stays for the next call.
    pub(crate) fn next_line(abort: &Abort) -> io::Result<LineRead> {
        let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
        line_from(libc::STDIN_FILENO, &mut pending, abort)
    }

    fn line_from(
        input: RawFd,
        pending: &mut Vec<u8>,
        abort: &Abort,
    ) -> io::Result<LineRead> {
        loop {
            // A stop wins over a line already there; the line stays.
            if abort.ended() {
                return Ok(LineRead::Aborted);
            }
            if let Some(line) = complete_line(pending) {
                return line.map(LineRead::Line);
            }
            if !input_ready(input, None, abort)? {
                return Ok(LineRead::Aborted);
            }
            let mut chunk = Zeroizing::new([0_u8; 4096]);
            match read_some(input, chunk.as_mut_slice()) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
                Ok(0) if pending.is_empty() => return Ok(LineRead::End),
                Ok(0) => {
                    return text_of(std::mem::take(pending)).map(LineRead::Line);
                }
                Ok(count) => {
                    pending.extend_from_slice(chunk.get(..count).unwrap_or_default());
                }
            }
        }
    }

    fn complete_line(pending: &mut Vec<u8>) -> Option<io::Result<String>> {
        let end = pending.iter().position(|byte| *byte == b'\n')?;
        let mut line: Vec<u8> = pending.drain(..=end).collect();
        line.pop();
        if pending.is_empty() {
            // shortcut: bytes of a longer rest stay where they were until the
            // buffer empties; a secret is wiped then.
            pending.zeroize();
        }
        Some(text_of(line))
    }

    fn text_of(bytes: Vec<u8>) -> io::Result<String> {
        String::from_utf8(bytes)
            .map(|mut line| {
                line.truncate(line.trim_end_matches('\r').len());
                line
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "stdin is not valid UTF-8")
            })
    }

    fn read_some(input: RawFd, into: &mut [u8]) -> io::Result<usize> {
        // SAFETY: `read` writes at most `into.len()` bytes into the slice.
        let count = unsafe { libc::read(input, into.as_mut_ptr().cast(), into.len()) };
        usize::try_from(count).map_err(|_| io::Error::last_os_error())
    }

    fn byte_from(
        input: RawFd,
        wait: Option<Duration>,
        abort: &Abort,
    ) -> io::Result<Option<u8>> {
        let give_up = wait.and_then(|wait| Instant::now().checked_add(wait));
        loop {
            if !input_ready(input, give_up, abort)? {
                return Ok(None);
            }
            match read_byte(input) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                read => return read.map(Some),
            }
        }
    }

    fn input_ready(
        input: RawFd,
        give_up: Option<Instant>,
        abort: &Abort,
    ) -> io::Result<bool> {
        loop {
            if abort.ended() {
                return Ok(false);
            }
            // The input first, then the pipe of every stop.
            let mut watched: Vec<libc::pollfd> = iter::once(input)
                .chain(abort.wake_fds())
                .map(|fd| libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let limit = [give_up, abort.deadline()].into_iter().flatten().min();
            let timeout = poll_timeout(limit, abort.has_dumb_stop());
            // SAFETY: `poll` reads and writes the array it is given.
            let ready = unsafe {
                libc::poll(
                    watched.as_mut_ptr(),
                    libc::nfds_t::try_from(watched.len()).unwrap_or(1),
                    timeout,
                )
            };
            if ready < 0 {
                let error = io::Error::last_os_error();
                // A signal the program handles cuts the wait, not the question.
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if watched.first().is_some_and(|first| first.revents != 0) {
                return Ok(true);
            }
            if give_up.is_some_and(|at| Instant::now() >= at) {
                return Ok(false);
            }
        }
    }

    /// Milliseconds `poll` waits: until `limit`, and at most a tick when a stop
    /// cannot wake it. Rounded up so it never spins on the last fraction.
    fn poll_timeout(limit: Option<Instant>, ticking: bool) -> libc::c_int {
        let until = limit.map(|at| at.saturating_duration_since(Instant::now()));
        let slice = match (until, ticking) {
            (Some(until), true) => Some(until.min(Abort::TICK)),
            (None, true) => Some(Abort::TICK),
            (until, false) => until,
        };
        slice.map_or(-1, |slice| {
            libc::c_int::try_from(slice.as_millis().saturating_add(1))
                .unwrap_or(libc::c_int::MAX)
        })
    }

    fn read_byte(input: RawFd) -> io::Result<u8> {
        let mut byte = 0_u8;
        // SAFETY: `read` writes at most one byte into the local.
        let count = unsafe { libc::read(input, ptr::from_mut(&mut byte).cast(), 1) };
        match count {
            1 => Ok(byte),
            0 => Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            _ => Err(io::Error::last_os_error()),
        }
    }

    #[must_use]
    pub(crate) fn columns() -> Option<usize> {
        let mut size = MaybeUninit::<libc::winsize>::zeroed();
        // SAFETY: `TIOCGWINSZ` only writes a `winsize` into a local.
        let status = unsafe {
            libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr())
        };
        (status == 0)
            // SAFETY: the call succeeded, so the value is initialised.
            .then(|| unsafe { size.assume_init() })
            .map(|size| usize::from(size.ws_col))
            .filter(|columns| *columns > 0)
    }

    #[cfg(test)]
    mod tests {
        use std::{
            io,
            os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd},
            thread,
            time::{Duration, Instant},
        };

        use super::{byte_from, line_from};
        use crate::terminal::{LineRead, abort::Abort};

        fn quiet_input() -> (OwnedFd, OwnedFd) {
            let mut fds = [0; 2];
            // SAFETY: `pipe` writes two descriptors into the array.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            // SAFETY: both descriptors are open and owned by nobody else.
            let [read, write] = fds.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
            (read, write)
        }

        fn type_x(typing: &OwnedFd) {
            type_bytes(typing, b"x");
        }

        fn type_bytes(typing: &OwnedFd, bytes: &[u8]) {
            // SAFETY: `write` reads `bytes.len()` bytes from the slice.
            let count =
                unsafe { libc::write(raw(typing), bytes.as_ptr().cast(), bytes.len()) };
            assert_eq!(usize::try_from(count).ok(), Some(bytes.len()));
        }

        fn raw(fd: &OwnedFd) -> RawFd {
            fd.as_raw_fd()
        }

        #[test]
        fn a_stop_from_another_thread_wakes_a_read_that_waits_forever() {
            let (input, _typing) = quiet_input();
            let (abort, stop) = Abort::new();
            let started = Instant::now();
            let stopper = thread::spawn(move || {
                thread::sleep(Duration::from_millis(30));
                stop.stop();
            });
            let byte = byte_from(raw(&input), None, &abort);
            assert!(stopper.join().is_ok());
            assert_eq!(byte.ok(), Some(None));
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn a_deadline_wakes_a_read_that_waits_forever() {
            let (input, _typing) = quiet_input();
            let abort = Abort::after(Duration::from_millis(30));
            let started = Instant::now();
            assert_eq!(byte_from(raw(&input), None, &abort).ok(), Some(None));
            assert!(started.elapsed() >= Duration::from_millis(30));
        }

        #[test]
        fn a_read_gives_up_after_its_own_wait() {
            let (input, _typing) = quiet_input();
            let wait = Some(Duration::from_millis(20));
            let byte = byte_from(raw(&input), wait, &Abort::default());
            assert_eq!(byte.ok(), Some(None));
        }

        #[test]
        fn a_typed_byte_is_read_while_nothing_ended() {
            let (input, typing) = quiet_input();
            let (abort, _stop) = Abort::new();
            type_x(&typing);
            assert_eq!(byte_from(raw(&input), None, &abort).ok(), Some(Some(b'x')));
        }

        extern "C" fn nothing(_: libc::c_int) {}

        #[test]
        #[expect(
            clippy::as_conversions,
            reason = "`sigaction` stores a handler as an integer"
        )]
        fn a_signal_that_cuts_the_wait_does_not_end_the_read() {
            let (input, typing) = quiet_input();
            let (sent, received) = std::sync::mpsc::channel();
            let (id_sent, id_received) = std::sync::mpsc::channel();
            // SAFETY: an all-zero `sigaction` is valid; the handler does
            // nothing.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            let handler: extern "C" fn(libc::c_int) = nothing;
            action.sa_sigaction = handler as usize;
            // SAFETY: both pointers lead to live locals.
            let status = unsafe {
                libc::sigaction(libc::SIGUSR2, &raw const action, std::ptr::null_mut())
            };
            assert_eq!(status, 0);
            let reader = thread::spawn(move || {
                // SAFETY: `pthread_self` takes nothing.
                id_sent
                    .send(unsafe { libc::pthread_self() })
                    .unwrap_or_default();
                sent.send(byte_from(raw(&input), None, &Abort::default()).ok())
                    .unwrap_or_default();
            });
            let id = id_received.recv_timeout(Duration::from_secs(5)).ok();
            thread::sleep(Duration::from_millis(50));
            if let Some(id) = id {
                // SAFETY: the reader thread is still running.
                unsafe { libc::pthread_kill(id, libc::SIGUSR2) };
            }
            thread::sleep(Duration::from_millis(50));
            let early = received.try_recv().is_err();
            type_x(&typing);
            let byte = received.recv_timeout(Duration::from_secs(5)).ok().flatten();
            assert!(reader.join().is_ok());
            assert_eq!((early, byte), (true, Some(Some(b'x'))));
        }

        #[test]
        fn a_stop_that_came_first_ends_the_read_before_it_looks_at_the_input() {
            let (input, typing) = quiet_input();
            let (abort, stop) = Abort::new();
            stop.stop();
            type_x(&typing);
            assert_eq!(byte_from(raw(&input), None, &abort).ok(), Some(None));
        }

        #[test]
        fn a_stop_from_another_thread_wakes_a_line_read_and_takes_no_partial_line() {
            let (input, typing) = quiet_input();
            let (abort, stop) = Abort::new();
            type_bytes(&typing, b"par");
            let mut pending = Vec::new();
            let started = Instant::now();
            let stopper = thread::spawn(move || {
                thread::sleep(Duration::from_millis(30));
                stop.stop();
            });
            let read = line_from(raw(&input), &mut pending, &abort);
            assert!(stopper.join().is_ok());
            assert_eq!(read.ok(), Some(LineRead::Aborted));
            assert!(started.elapsed() < Duration::from_secs(5));
            type_bytes(&typing, b"tial\n");
            let again = line_from(raw(&input), &mut pending, &Abort::default());
            assert_eq!(again.ok(), Some(LineRead::Line("partial".to_owned())));
        }

        #[test]
        fn a_deadline_wakes_a_line_read_that_waits_forever() {
            let (input, _typing) = quiet_input();
            let abort = Abort::after(Duration::from_millis(30));
            let started = Instant::now();
            let read = line_from(raw(&input), &mut Vec::new(), &abort);
            assert_eq!(read.ok(), Some(LineRead::Aborted));
            assert!(started.elapsed() >= Duration::from_millis(30));
        }

        #[test]
        fn lines_arrive_one_by_one_also_when_they_came_together() {
            let (input, typing) = quiet_input();
            type_bytes(&typing, b"one\r\ntwo\nlast");
            drop(typing);
            let mut pending = Vec::new();
            let mut next = || line_from(raw(&input), &mut pending, &Abort::default());
            assert_eq!(next().ok(), Some(LineRead::Line("one".to_owned())));
            assert_eq!(next().ok(), Some(LineRead::Line("two".to_owned())));
            assert_eq!(next().ok(), Some(LineRead::Line("last".to_owned())));
            assert_eq!(next().ok(), Some(LineRead::End));
        }

        #[test]
        fn a_stop_that_came_first_leaves_a_waiting_line_unread() {
            let (input, typing) = quiet_input();
            let (abort, stop) = Abort::new();
            type_bytes(&typing, b"hello\n");
            stop.stop();
            let mut pending = Vec::new();
            let read = line_from(raw(&input), &mut pending, &abort);
            assert_eq!(read.ok(), Some(LineRead::Aborted));
            let later = line_from(raw(&input), &mut pending, &Abort::default());
            assert_eq!(later.ok(), Some(LineRead::Line("hello".to_owned())));
        }

        #[test]
        fn a_line_that_is_no_utf8_is_an_error() {
            let (input, typing) = quiet_input();
            type_bytes(&typing, b"\xff\n");
            let read = line_from(raw(&input), &mut Vec::new(), &Abort::default());
            assert_eq!(
                read.err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidData)
            );
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::{io, ptr, sync::OnceLock};

    use windows_sys::{
        Win32::{
            Foundation::{HANDLE, INVALID_HANDLE_VALUE},
            System::Console::{
                ENABLE_ECHO_INPUT,
                ENABLE_LINE_INPUT,
                ENABLE_PROCESSED_INPUT,
                FlushConsoleInputBuffer,
                GetConsoleMode,
                GetStdHandle,
                STD_HANDLE,
                STD_INPUT_HANDLE,
                SetConsoleCtrlHandler,
                SetConsoleMode,
            },
        },
        core::BOOL,
    };

    use crate::terminal::tty::TerminalQuiet;

    pub(super) type TerminalSettings = u32;

    static ORIGINAL: OnceLock<TerminalSettings> = OnceLock::new();

    pub(crate) fn handle(which: STD_HANDLE) -> io::Result<HANDLE> {
        // SAFETY: `GetStdHandle` takes a constant and returns a handle or an
        // invalid one.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(handle)
        }
    }

    pub(crate) fn mode_of(handle: HANDLE) -> io::Result<u32> {
        let mut mode = 0;
        // SAFETY: `GetConsoleMode` only writes the mode into a local.
        if unsafe { GetConsoleMode(handle, ptr::from_mut(&mut mode)) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(mode)
        }
    }

    pub(crate) fn set_mode(handle: HANDLE, mode: u32) -> io::Result<()> {
        // SAFETY: `SetConsoleMode` takes a handle and flags by value.
        if unsafe { SetConsoleMode(handle, mode) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn stdin_mode() -> io::Result<TerminalSettings> {
        handle(STD_INPUT_HANDLE).and_then(mode_of)
    }

    pub(super) fn remember() {
        if let Ok(mode) = stdin_mode() {
            ORIGINAL.get_or_init(|| {
                // SAFETY: the handler only calls `SetConsoleMode`; failure is
                // ignored.
                unsafe { SetConsoleCtrlHandler(Some(on_event), 1) };
                mode
            });
        }
    }

    /// Runs on a thread of its own. Returning 0 passes the event on, so the
    /// process ends as it would have.
    unsafe extern "system" fn on_event(_: u32) -> BOOL {
        drop(restore());
        0
    }

    pub(super) fn restore() -> Option<String> {
        ORIGINAL.get().and_then(|mode| put_back(*mode))
    }

    pub(super) fn put_back(mode: TerminalSettings) -> Option<String> {
        stdin_handle_set(mode).err().map(|error| {
            format!("warning: the terminal settings were not put back: {error}")
        })
    }

    fn stdin_handle_set(mode: TerminalSettings) -> io::Result<()> {
        handle(STD_INPUT_HANDLE).and_then(|stdin| set_mode(stdin, mode))
    }

    pub(super) fn current() -> io::Result<TerminalSettings> {
        stdin_mode()
    }

    pub(super) fn apply(
        original: TerminalSettings,
        level: Option<TerminalQuiet>,
    ) -> io::Result<()> {
        // The console refuses an echo that stays on without line input, so the
        // three go together.
        let off = match level {
            None => 0,
            Some(TerminalQuiet::Echo) => ENABLE_ECHO_INPUT,
            Some(TerminalQuiet::Keys) => {
                ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT
            }
        };
        stdin_handle_set(original & !off)
    }

    pub(super) fn discard_typeahead() {
        if let Ok(stdin) = handle(STD_INPUT_HANDLE) {
            // SAFETY: `FlushConsoleInputBuffer` takes a handle; failure is
            // ignored.
            unsafe { FlushConsoleInputBuffer(stdin) };
        }
    }

    pub(super) fn flush_output() {}
}
