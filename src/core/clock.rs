//! Time and racing for the async API, on std alone, so any executor drives it.

use std::{
    future::poll_fn,
    pin::pin,
    sync::{
        Arc,
        Mutex,
        PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    task::{Poll, Waker},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
struct Shared {
    due: AtomicBool,
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Shared {
    fn fire(&self) {
        self.due.store(true, Ordering::Release);
        let waker = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        waker.into_iter().for_each(Waker::wake);
    }
}

/// A tick every `period`, from a helper thread that lives as long as the
/// ticker; late ticks merge into one.
#[derive(Debug)]
pub(crate) struct Ticker {
    shared: Arc<Shared>,
    thread: Option<thread::Thread>,
}

impl Ticker {
    pub(crate) fn every(period: Duration) -> Self {
        let shared = Arc::new(Shared::default());
        let beat = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("clacky-ticker".to_owned())
            .spawn(move || {
                let mut next = Instant::now() + period;
                while !beat.stopped.load(Ordering::Acquire) {
                    if let Some(left) = next.checked_duration_since(Instant::now()) {
                        thread::park_timeout(left);
                    } else {
                        beat.fire();
                        next = Instant::now() + period;
                    }
                }
            })
            .map(|handle| handle.thread().clone())
            .ok();
        Self { shared, thread }
    }

    /// A ticker with no thread; the [`Pulse`] ticks it.
    #[cfg(test)]
    pub(crate) fn manual() -> (Self, Pulse) {
        let shared = Arc::new(Shared::default());
        (
            Self {
                shared: Arc::clone(&shared),
                thread: None,
            },
            Pulse(shared),
        )
    }

    /// Resolves on the next tick, or at once when one is already due.
    pub(crate) fn tick(&self) -> impl Future<Output = ()> {
        poll_fn(|cx| {
            let shared = &self.shared;
            if shared.due.swap(false, Ordering::AcqRel) {
                return Poll::Ready(());
            }
            *shared.waker.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(cx.waker().clone());
            // A tick between the swap and the waker store is not lost.
            if shared.due.swap(false, Ordering::AcqRel) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.thread.iter().for_each(thread::Thread::unpark);
    }
}

/// The hand that ticks a [`Ticker::manual`].
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct Pulse(Arc<Shared>);

#[cfg(test)]
impl Pulse {
    pub(crate) fn fire(&self) {
        self.0.fire();
    }
}

/// `Some` with what `first` gives, or `None` when `second` is ready before it;
/// `first` is polled first, and the loser is dropped.
pub(crate) async fn race<A: Future>(
    first: A,
    second: impl Future<Output = ()>,
) -> Option<A::Output> {
    let (mut first, mut second) = (pin!(first), pin!(second));
    poll_fn(|cx| match first.as_mut().poll(cx) {
        Poll::Ready(out) => Poll::Ready(Some(out)),
        Poll::Pending => second.as_mut().poll(cx).map(|()| None),
    })
    .await
}

/// Runs `future` to its end on this thread, parked while it waits.
#[cfg(test)]
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Wake};

    struct Unpark(thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        thread::park();
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    #[test]
    fn a_ticker_ticks_on_its_own_thread() {
        let ticker = Ticker::every(Duration::from_millis(2));
        block_on(ticker.tick());
        block_on(ticker.tick());
    }

    #[test]
    fn dropping_a_ticker_ends_its_thread() {
        let ticker = Ticker::every(Duration::from_secs(3600));
        let shared = Arc::downgrade(&ticker.shared);
        drop(ticker);
        let ended = (0..200).any(|_| {
            thread::sleep(Duration::from_millis(1));
            shared.strong_count() == 0
        });
        assert!(ended);
    }

    #[test]
    fn ticks_that_come_late_merge_into_one() {
        let (ticker, pulse) = Ticker::manual();
        pulse.fire();
        pulse.fire();
        block_on(ticker.tick());
        let second = block_on(race(ticker.tick(), async {}));
        assert_eq!(second, None);
    }

    #[test]
    fn the_first_future_wins_a_tie() {
        let both = block_on(race(async { 1 }, async {}));
        let second = block_on(race(pending::<u8>(), async {}));
        assert_eq!((both, second), (Some(1), None));
    }
}
