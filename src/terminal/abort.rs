//! [`Abort`] and [`Stop`]: ending a question from another thread or at a
//! deadline.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default)]
/// What ends a question from outside: a [`Stop`] called or a deadline reached.
/// The default never ends.
pub struct Abort {
    stops: Vec<Arc<Flag>>,
    deadline: Option<Instant>,
}

#[derive(Debug, Clone)]
/// Ends the questions that carry its [`Abort`].
pub struct Stop(Arc<Flag>);

#[derive(Debug)]
struct Flag {
    stopped: AtomicBool,
    /// Wakes a blocked key read; `None` when the system gave none, and the read
    /// then looks every [`Abort::TICK`].
    wake: Option<wake::Wake>,
}

impl Abort {
    #[cfg(any(unix, windows))]
    pub(super) const TICK: Duration = Duration::from_millis(50);

    /// An abort and the [`Stop`] that ends it.
    #[must_use]
    pub fn new() -> (Self, Stop) {
        let flag = Arc::new(Flag {
            stopped: AtomicBool::new(false),
            wake: wake::Wake::new(),
        });
        (
            Self {
                stops: vec![Arc::clone(&flag)],
                deadline: None,
            },
            Stop(flag),
        )
    }

    #[must_use]
    pub(crate) fn after(wait: Duration) -> Self {
        Self {
            stops: Vec::new(),
            deadline: Instant::now().checked_add(wait),
        }
    }

    #[must_use]
    pub(crate) fn both(&self, other: &Self) -> Self {
        let mut stops = self.stops.clone();
        for flag in &other.stops {
            if !stops.iter().any(|known| Arc::ptr_eq(known, flag)) {
                stops.push(Arc::clone(flag));
            }
        }
        Self {
            stops,
            deadline: [self.deadline, other.deadline].into_iter().flatten().min(),
        }
    }

    /// Whether a stop was called or the deadline has passed.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.stops
            .iter()
            .any(|flag| flag.stopped.load(Ordering::Acquire))
            || self.deadline.is_some_and(|at| Instant::now() >= at)
    }

    #[must_use]
    pub(crate) const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    #[cfg(any(test, feature = "fake"))]
    #[must_use]
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    #[cfg(any(test, feature = "fake"))]
    pub(crate) fn fire(&self) {
        self.stops.iter().for_each(|flag| flag.raise());
    }

    #[cfg(any(unix, windows))]
    pub(super) fn has_dumb_stop(&self) -> bool {
        self.stops.iter().any(|flag| flag.wake.is_none())
    }
}

impl Flag {
    fn raise(&self) {
        let first = !self.stopped.swap(true, Ordering::AcqRel);
        self.wake
            .as_ref()
            .filter(|_| first)
            .iter()
            .for_each(|wake| wake.raise());
    }
}

impl Stop {
    /// Ends every question that carries the matching [`Abort`], now and later.
    pub fn stop(&self) {
        self.0.raise();
    }
}

#[cfg(unix)]
impl Abort {
    pub(super) fn wake_fds(&self) -> impl Iterator<Item = std::os::fd::RawFd> + '_ {
        self.stops
            .iter()
            .filter_map(|flag| flag.wake.as_ref().map(wake::Wake::fd))
    }
}

#[cfg(windows)]
impl Abort {
    pub(super) fn wake_handles(
        &self,
    ) -> impl Iterator<Item = windows_sys::Win32::Foundation::HANDLE> + '_ {
        self.stops
            .iter()
            .filter_map(|flag| flag.wake.as_ref().map(wake::Wake::handle))
    }
}

/// A pipe that is written to once, never read: readable ever after.
#[cfg(unix)]
mod wake {
    use std::{
        os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd},
        ptr,
    };

    #[derive(Debug)]
    pub(super) struct Wake {
        read: OwnedFd,
        write: OwnedFd,
    }

    impl Wake {
        pub(super) fn new() -> Option<Self> {
            let mut fds = [0; 2];
            // SAFETY: `pipe` writes two descriptors into the array.
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                return None;
            }
            // SAFETY: the call succeeded, so both descriptors are open and
            // owned by nobody else.
            let [read, write] = fds.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
            let set = |fd: &OwnedFd,
                       get: libc::c_int,
                       put: libc::c_int,
                       flag: libc::c_int| {
                // SAFETY: `fcntl` takes a descriptor and flags by value.
                unsafe {
                    let before = libc::fcntl(fd.as_raw_fd(), get);
                    before >= 0 && libc::fcntl(fd.as_raw_fd(), put, before | flag) == 0
                }
            };
            // The stop never blocks and no child process inherits the pipe.
            (set(&read, libc::F_GETFD, libc::F_SETFD, libc::FD_CLOEXEC)
                && set(&write, libc::F_GETFD, libc::F_SETFD, libc::FD_CLOEXEC)
                && set(&write, libc::F_GETFL, libc::F_SETFL, libc::O_NONBLOCK))
            .then_some(Self { read, write })
        }

        pub(super) fn raise(&self) {
            let byte = 1_u8;
            // SAFETY: `write` reads one byte from the local; a full pipe is
            // already readable, so a failure is ignored.
            unsafe {
                libc::write(self.write.as_raw_fd(), ptr::from_ref(&byte).cast(), 1)
            };
        }

        pub(super) fn fd(&self) -> RawFd {
            self.read.as_raw_fd()
        }
    }
}

/// A manual-reset event that is set once: signalled ever after.
#[cfg(windows)]
mod wake {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{CreateEventW, SetEvent},
    };

    #[derive(Debug)]
    pub(super) struct Wake(HANDLE);

    // SAFETY: an event handle may be used from any thread.
    unsafe impl Send for Wake {}
    // SAFETY: as above; `SetEvent` and the waits are thread-safe.
    unsafe impl Sync for Wake {}

    impl Wake {
        pub(super) fn new() -> Option<Self> {
            // SAFETY: `CreateEventW` takes no pointers but the null defaults.
            let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            (!event.is_null()).then_some(Self(event))
        }

        pub(super) fn raise(&self) {
            // SAFETY: the handle is open until `drop`; failure is ignored.
            unsafe { SetEvent(self.0) };
        }

        pub(super) const fn handle(&self) -> HANDLE {
            self.0
        }
    }

    // GUARD: closes the event that `new` made.
    impl Drop for Wake {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by `self`.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::Abort;

    #[test]
    fn a_new_abort_has_not_ended_and_a_stop_ends_it() {
        let (abort, stop) = Abort::new();
        assert!(!abort.ended());
        stop.stop();
        stop.stop();
        assert!(abort.ended());
    }

    #[test]
    fn a_stop_ends_every_copy_and_every_union() {
        let (abort, stop) = Abort::new();
        let (other, _kept) = Abort::new();
        let (both, copy) = (other.both(&abort), abort.clone());
        stop.stop();
        assert!(abort.ended() && copy.ended() && both.ended() && !other.ended());
    }

    #[test]
    fn a_deadline_ends_it_and_the_sooner_one_wins() {
        let soon = Abort::after(Duration::ZERO);
        let late = Abort::after(Duration::from_secs(3600));
        assert!(soon.ended() && !late.ended());
        assert!(late.both(&soon).ended());
        assert!(!Abort::default().ended());
    }

    #[test]
    fn a_stop_works_from_another_thread() {
        let (abort, stop) = Abort::new();
        assert!(thread::spawn(move || stop.stop()).join().is_ok());
        assert!(abort.ended());
    }
}
