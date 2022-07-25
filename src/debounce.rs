use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Future, Stream};
use pin_project::pin_project;
use tokio::time::{sleep, Duration, Sleep};

#[pin_project]
pub struct Debounce<S: Stream> {
    #[pin]
    inner: S,
    #[pin]
    state: State,
    queue: VecDeque<S::Item>,
    duration: Duration,
}

#[pin_project(project = DebounceStateProj)]
#[allow(clippy::large_enum_variant)]
enum State {
    Debouncing(#[pin] Sleep),
    WaitingForInner,
}

impl<S: Stream> Debounce<S> {
    pub fn new(inner: S, duration: Duration) -> Self {
        Self {
            inner,
            state: State::WaitingForInner,
            queue: VecDeque::new(),
            duration,
        }
    }
}

impl<S: Stream> Stream for Debounce<S>
where
    S::Item: Clone,
{
    type Item = VecDeque<S::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        let queue = this.queue;

        loop {
            match this.inner.as_mut().poll_next(cx) {
                // inner stream ended => drain queue or hang up
                Poll::Ready(None) => {
                    return if !queue.is_empty() {
                        Poll::Ready(Some(queue.drain(..).collect()))
                    } else {
                        Poll::Ready(None)
                    };
                }
                // inner stream produced an item => queue it and reset sleep
                Poll::Ready(Some(item)) => {
                    queue.push_back(item);
                    this.state.set(State::Debouncing(sleep(*this.duration)));
                }
                // inner stream chilling => check state
                Poll::Pending => {
                    return match this.state.as_mut().project() {
                        // waiting for inner stream => chill
                        DebounceStateProj::WaitingForInner => Poll::Pending,
                        // debouncing => poll the sleep future
                        DebounceStateProj::Debouncing(mut debounce_sleep) => {
                            match debounce_sleep.as_mut().poll(cx) {
                                // debounce sleep done => drain
                                Poll::Ready(()) => {
                                    this.state.set(State::WaitingForInner);
                                    Poll::Ready(Some(queue.drain(..).collect()))
                                }
                                // still sleeping => chill
                                Poll::Pending => Poll::Pending,
                            }
                        }
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Debounce;
    use futures::{channel::mpsc, pin_mut, poll, stream, StreamExt};
    use std::{collections::VecDeque, task::Poll, time::Duration};

    #[tokio::test]
    async fn should_give_up() {
        tokio::time::pause();
        let rx = stream::empty::<()>();
        let deb = Debounce::new(rx, Duration::from_millis(100));
        pin_mut!(deb);

        tokio::time::advance(Duration::from_millis(90)).await;
        assert_eq!(poll!(deb.next()), Poll::Ready(None));
        tokio::time::advance(Duration::from_millis(90)).await;
        assert_eq!(poll!(deb.next()), Poll::Ready(None));
    }

    #[tokio::test]
    async fn should_drain() {
        tokio::time::pause();
        let rx = stream::iter([1, 2, 3, 4, 5]);
        let deb = Debounce::new(rx, Duration::from_millis(100));
        pin_mut!(deb);

        assert_eq!(
            poll!(deb.next()),
            Poll::Ready(Some(VecDeque::from([1, 2, 3, 4, 5])))
        );
    }

    #[tokio::test]
    async fn should_debounce() {
        tokio::time::pause();
        let (tx, rx) = mpsc::unbounded();

        let deb = Debounce::new(rx, Duration::from_millis(100));
        pin_mut!(deb);

        tx.unbounded_send(1).unwrap();
        tx.unbounded_send(2).unwrap();
        tx.unbounded_send(3).unwrap();
        assert_eq!(poll!(deb.next()), Poll::Pending);

        tokio::time::advance(Duration::from_millis(110)).await;
        assert_eq!(
            poll!(deb.next()),
            Poll::Ready(Some(VecDeque::from([1, 2, 3])))
        );

        tx.unbounded_send(4).unwrap();
        tx.unbounded_send(5).unwrap();
        assert_eq!(poll!(deb.next()), Poll::Pending);

        tokio::time::advance(Duration::from_millis(90)).await;
        assert_eq!(poll!(deb.next()), Poll::Pending);

        tokio::time::advance(Duration::from_millis(20)).await;
        assert_eq!(poll!(deb.next()), Poll::Ready(Some(VecDeque::from([4, 5]))));

        tx.unbounded_send(6).unwrap();
        tx.close_channel();
        assert_eq!(poll!(deb.next()), Poll::Ready(Some(VecDeque::from([6]))));
    }
}
