//! Windows console input, width and escape support.

use std::{
    io,
    ptr,
    sync::{
        Mutex,
        PoisonError,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0},
    System::{
        Console::{
            CONSOLE_SCREEN_BUFFER_INFO,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            GetConsoleScreenBufferInfo,
            INPUT_RECORD,
            KEY_EVENT,
            KEY_EVENT_RECORD,
            ReadConsoleInputW,
            STD_ERROR_HANDLE,
            STD_INPUT_HANDLE,
        },
        Threading::{INFINITE, WaitForMultipleObjects},
    },
};

use crate::terminal::{
    LineRead,
    Press,
    abort::Abort,
    console_keys::{KeyInput, keystroke_of},
    tty::{handle, mode_of, set_mode},
};

/// Switches the console to show escapes and says whether stderr does. A stream
/// that is no console, such as a mintty pty, counts as showing them.
pub(super) fn enable_ansi_on_stderr() -> bool {
    handle(STD_ERROR_HANDLE).is_ok_and(|stderr| {
        mode_of(stderr).map_or(true, |mode| {
            set_mode(stderr, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).is_ok()
        })
    })
}

pub(super) fn columns() -> Option<usize> {
    let stderr = handle(STD_ERROR_HANDLE).ok()?;
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: `GetConsoleScreenBufferInfo` only writes into a local.
    let status =
        unsafe { GetConsoleScreenBufferInfo(stderr, ptr::from_mut(&mut info)) };
    let window = info.srWindow;
    (status != 0)
        .then(|| i32::from(window.Right) - i32::from(window.Left) + 1)
        .and_then(|columns| usize::try_from(columns).ok())
        .filter(|columns| *columns > 0)
}

/// The next key press, skipping keys that mean nothing here, or
/// [`Press::Aborted`] if `abort` ended first.
pub(super) fn read_keystroke(abort: &Abort) -> io::Result<Press> {
    let stdin = handle(STD_INPUT_HANDLE)?;
    loop {
        let Some(first) = read_key_input(stdin, abort)? else {
            return Ok(Press::Aborted);
        };
        // The rest of a key is already in the buffer: nothing there to end.
        let rest = || {
            read_key_input(stdin, &Abort::default())?
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
        };
        if let Some(keystroke) = keystroke_of(first, rest)? {
            return Ok(Press::Key(keystroke));
        }
    }
}

fn read_key_input(stdin: HANDLE, abort: &Abort) -> io::Result<Option<KeyInput>> {
    loop {
        if !wait_for_input(stdin, abort)? {
            return Ok(None);
        }
        let mut record = INPUT_RECORD::default();
        let mut read = 0;
        // SAFETY: `ReadConsoleInputW` writes one record and a count into
        // locals; the console has a record, so it does not block.
        if unsafe {
            ReadConsoleInputW(
                stdin,
                ptr::from_mut(&mut record),
                1,
                ptr::from_mut(&mut read),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if u32::from(record.EventType) == KEY_EVENT {
            // SAFETY: the record is a key event, so that union field is the one
            // set.
            let key = unsafe { record.Event.KeyEvent };
            if key.bKeyDown != 0 {
                return Ok(Some(input_of(&key)));
            }
        }
    }
}

fn wait_for_input(stdin: HANDLE, abort: &Abort) -> io::Result<bool> {
    loop {
        if abort.ended() {
            return Ok(false);
        }
        // The console first, then the event of every stop; one wait takes 64
        // handles at most, so the stops beyond them are looked at every tick.
        let stops: Vec<HANDLE> = abort.wake_handles().collect();
        let watched: Vec<HANDLE> = std::iter::once(stdin)
            .chain(stops.iter().copied().take(MAX_STOPS))
            .collect();
        let unwatched = stops.len() > MAX_STOPS;
        let limit = abort
            .deadline()
            .map(|at| at.saturating_duration_since(Instant::now()));
        let limit = if unwatched || abort.has_dumb_stop() {
            Some(limit.map_or(Abort::TICK, |limit| limit.min(Abort::TICK)))
        } else {
            limit
        };
        // SAFETY: `WaitForMultipleObjects` reads the array of open handles.
        let status = unsafe {
            WaitForMultipleObjects(
                u32::try_from(watched.len()).unwrap_or(1),
                watched.as_ptr(),
                0,
                limit.map_or(INFINITE, millis),
            )
        };
        match status {
            WAIT_OBJECT_0 => return Ok(true),
            WAIT_FAILED => return Err(io::Error::last_os_error()),
            // A stop's event, or the time ran out: the check above decides.
            _ => {}
        }
    }
}

/// The next line of stdin, or [`LineRead::Aborted`] if `abort` ended first. A
/// thread reads it, because a handle of piped input cannot be waited on for a
/// line. A read that `abort` cut short goes on in its thread, and the next call
/// takes its line.
pub(super) fn read_line(abort: &Abort) -> io::Result<LineRead> {
    static READING: Mutex<Option<Receiver<io::Result<Option<String>>>>> =
        Mutex::new(None);
    let mut reading = READING.lock().unwrap_or_else(PoisonError::into_inner);
    let waiting = if let Some(waiting) = reading.take() {
        waiting
    } else {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("clacky-stdin".to_owned())
            .spawn(move || {
                let line = io::stdin().lines().next().transpose().map(|line| {
                    line.map(|mut line| {
                        line.truncate(line.trim_end_matches('\r').len());
                        line
                    })
                });
                sender.send(line).unwrap_or_default();
            })?;
        receiver
    };
    loop {
        // A stop wins over a line that is ready; the line stays for later.
        if abort.ended() {
            *reading = Some(waiting);
            return Ok(LineRead::Aborted);
        }
        let limit = abort.deadline().map_or(Abort::TICK, |at| {
            at.saturating_duration_since(Instant::now())
                .min(Abort::TICK)
        });
        match waiting.recv_timeout(limit) {
            Ok(line) => {
                return line.map(|line| line.map_or(LineRead::End, LineRead::Line));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
        }
    }
}

/// How many stops one wait watches: the handle limit of 64, less stdin.
const MAX_STOPS: usize = 63;

fn millis(wait: Duration) -> u32 {
    u32::try_from(wait.as_millis().saturating_add(1)).unwrap_or(u32::MAX - 1)
}

fn input_of(event: &KEY_EVENT_RECORD) -> KeyInput {
    KeyInput {
        // SAFETY: `ReadConsoleInputW` fills the character as UTF-16.
        unit: unsafe { event.uChar.UnicodeChar },
        code: event.wVirtualKeyCode,
        state: event.dwControlKeyState,
    }
}
