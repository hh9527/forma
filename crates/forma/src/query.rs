use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

#[derive(Clone, Debug, Default)]
pub struct RevisionClock {
    current: Arc<AtomicU64>,
}

impl RevisionClock {
    pub fn current(&self) -> Revision {
        Revision(self.current.load(Ordering::Acquire))
    }

    pub fn advance(&self) -> Revision {
        Revision(self.current.fetch_add(1, Ordering::AcqRel) + 1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct QueryContext {
    revision: Revision,
    clock: RevisionClock,
    cancellation: CancellationToken,
}

impl QueryContext {
    pub fn new(revision: Revision, clock: RevisionClock, cancellation: CancellationToken) -> Self {
        Self {
            revision,
            clock,
            cancellation,
        }
    }

    pub fn current(clock: RevisionClock) -> Self {
        let revision = clock.current();
        Self::new(revision, clock, CancellationToken::default())
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub fn check(&self) -> Result<(), QueryError> {
        if self.cancellation.is_cancelled() {
            return Err(QueryError::Cancelled);
        }
        let current = self.clock.current();
        if current != self.revision {
            return Err(QueryError::StaleRevision {
                requested: self.revision,
                current,
            });
        }
        Ok(())
    }

    pub async fn checkpoint(&self) -> Result<(), QueryError> {
        self.check()?;
        YieldOnce::default().await;
        self.check()
    }

    pub fn ensure_snapshot(&self, revision: Revision) -> Result<(), QueryError> {
        self.check()?;
        if revision != self.revision {
            return Err(QueryError::SnapshotRevision {
                requested: self.revision,
                snapshot: revision,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError {
    Cancelled,
    StaleRevision {
        requested: Revision,
        current: Revision,
    },
    SnapshotRevision {
        requested: Revision,
        snapshot: Revision,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("query cancelled"),
            Self::StaleRevision { requested, current } => write!(
                formatter,
                "query revision {} is stale; current revision is {}",
                requested.0, current.0
            ),
            Self::SnapshotRevision {
                requested,
                snapshot,
            } => write!(
                formatter,
                "query revision {} does not match snapshot revision {}",
                requested.0, snapshot.0
            ),
        }
    }
}

impl std::error::Error for QueryError {}

#[derive(Default)]
struct YieldOnce {
    yielded: bool,
}

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use super::*;

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
                return result;
            }
        }
    }

    #[test]
    fn distinguishes_explicit_cancellation_from_stale_revisions() {
        let clock = RevisionClock::default();
        let cancellation = CancellationToken::default();
        let context = QueryContext::new(clock.current(), clock.clone(), cancellation.clone());
        assert_eq!(block_on(context.checkpoint()), Ok(()));

        cancellation.cancel();
        assert_eq!(block_on(context.checkpoint()), Err(QueryError::Cancelled));

        let context = QueryContext::current(clock.clone());
        let requested = context.revision();
        let current = clock.advance();
        assert_eq!(
            block_on(context.checkpoint()),
            Err(QueryError::StaleRevision { requested, current })
        );
    }
}
