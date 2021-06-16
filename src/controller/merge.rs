use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use super::Event;
use crate::controller::event::EventType;
use futures::{ready, Stream};
use pin_project_lite::pin_project;
use tokio::time::{sleep, Instant, Sleep};

pin_project! {
    pub struct MergeEvent<S> {
        #[pin]
        inner: S,
        #[pin]
        timer: Sleep,
        queue: VecDeque<Event>,
    }
}

impl<S> Stream for MergeEvent<S>
where
    S: Stream<Item = Event> + Unpin,
{
    type Item = Event;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if this.timer.is_elapsed() {
            if this.queue.len() > 0 {
                return Poll::Ready(this.queue.pop_front());
            }
            this.timer
                .reset(Instant::now() + Duration::from_millis(100));
        } else {
            let timer = this.timer.poll(cx);
            if let Poll::Ready(_) = timer {
                return Poll::Ready(_);
            }
        }

        let item = ready!(this.inner.poll_next(cx));

        if let Some(item) = item {
            if item.event_type.mergeable() {
                this.queue.push_back(item);
            } else {
                return Poll::Ready(Some(item));
            }
        }

        todo!()
    }
}

impl<S> MergeEvent<S> {
    pub fn new(inner: S) -> Self {
        MergeEvent {
            inner,
            timer: sleep(Duration::from_secs(0)),
            queue: VecDeque::new(),
        }
    }
}
