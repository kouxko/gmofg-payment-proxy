use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::deterministic_rng::DeterministicRng;
use super::{JitterScope, TrafficSchedule};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacedBodyError {
    Cancelled,
    Disconnected,
}

impl Display for PacedBodyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "weak-network body cancelled",
            Self::Disconnected => "weak-network body intentionally disconnected",
        })
    }
}

impl std::error::Error for PacedBodyError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitOutcome {
    Ready,
    Cancelled,
}

type WaitFuture = Pin<Box<dyn Future<Output = WaitOutcome> + Send>>;

pub struct PacedBody {
    data: Bytes,
    offset: usize,
    claimed_length: u64,
    schedule: TrafficSchedule,
    cancellation: CancellationToken,
    rng: DeterministicRng,
    wait: Option<WaitFuture>,
    started_at: Instant,
    next_throttle_at: Instant,
    before_message_waited: bool,
    jittered_offset: Option<usize>,
    disconnect_reported: bool,
}

impl Debug for PacedBody {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PacedBody")
            .field("offset", &self.offset)
            .field("length", &self.data.len())
            .field("claimed_length", &self.claimed_length)
            .field("schedule", &self.schedule)
            .finish_non_exhaustive()
    }
}

impl PacedBody {
    #[must_use]
    pub fn new(
        data: Bytes,
        claimed_length: usize,
        schedule: TrafficSchedule,
        cancellation: CancellationToken,
    ) -> Self {
        let now = Instant::now();
        let seed = schedule.seed;
        Self {
            data,
            offset: 0,
            claimed_length: u64::try_from(claimed_length).unwrap_or(u64::MAX),
            schedule,
            cancellation,
            rng: DeterministicRng::new(seed),
            wait: None,
            started_at: now,
            next_throttle_at: now,
            before_message_waited: false,
            jittered_offset: None,
            disconnect_reported: false,
        }
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.offset >= self.data.len() && self.disconnect_reported
            || self.offset >= self.data.len() && self.schedule.disconnect_after_bytes.is_none()
    }

    fn schedule_wait(&mut self, deadline: Instant) {
        let cancellation = self.cancellation.clone();
        self.wait = Some(Box::pin(async move {
            tokio::select! {
                () = cancellation.cancelled() => WaitOutcome::Cancelled,
                () = tokio::time::sleep_until(deadline) => WaitOutcome::Ready,
            }
        }));
    }

    fn poll_wait(&mut self, context: &mut Context<'_>) -> Poll<Result<(), PacedBodyError>> {
        let Some(wait) = self.wait.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match wait.as_mut().poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(WaitOutcome::Ready) => {
                self.wait = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(WaitOutcome::Cancelled) => {
                self.wait = None;
                Poll::Ready(Err(PacedBodyError::Cancelled))
            }
        }
    }

    fn jitter_duration(&mut self) -> Duration {
        self.schedule.jitter.map_or(Duration::ZERO, |profile| {
            Duration::from_millis(self.rng.range_inclusive(
                profile.minimum.as_millis().try_into().unwrap_or(u64::MAX),
                profile.maximum.as_millis().try_into().unwrap_or(u64::MAX),
            ))
        })
    }

    fn next_wait_deadline(&mut self) -> Option<Instant> {
        let now = Instant::now();
        let mut deadline = now;

        if !self.before_message_waited
            && self
                .schedule
                .jitter
                .is_some_and(|profile| matches!(profile.scope, JitterScope::BeforeMessage))
        {
            self.before_message_waited = true;
            deadline += self.jitter_duration();
        }

        if self.jittered_offset != Some(self.offset)
            && self
                .schedule
                .jitter
                .is_some_and(|profile| matches!(profile.scope, JitterScope::PerChunk))
        {
            self.jittered_offset = Some(self.offset);
            deadline += self.jitter_duration();
        }

        if let Some(profile) = self.schedule.intermittent {
            let cycle = profile.available.saturating_add(profile.blocked);
            if !cycle.is_zero() {
                let elapsed = now.duration_since(self.started_at);
                let cycle_nanos = cycle.as_nanos();
                let phase_nanos = elapsed.as_nanos() % cycle_nanos;
                if phase_nanos >= profile.available.as_nanos() {
                    let remaining_nanos = cycle_nanos - phase_nanos;
                    let remaining =
                        Duration::from_nanos(u64::try_from(remaining_nanos).unwrap_or(u64::MAX));
                    deadline = deadline.max(now + remaining);
                }
            }
        }

        (deadline > now).then_some(deadline)
    }

    fn next_chunk_len(&self) -> usize {
        let mut end = self
            .offset
            .saturating_add(self.schedule.chunk_bytes(self.data.len()))
            .min(self.data.len());
        if let Some(disconnect_after) = self.schedule.disconnect_after_bytes {
            end = end.min(disconnect_after);
        }
        end.saturating_sub(self.offset)
    }
}

impl Body for PacedBody {
    type Data = Bytes;
    type Error = PacedBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.cancellation.is_cancelled() {
            return Poll::Ready(Some(Err(PacedBodyError::Cancelled)));
        }
        match self.poll_wait(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok(())) => {}
        }

        if let Some(disconnect_after) = self.schedule.disconnect_after_bytes
            && self.offset >= disconnect_after
            && !self.disconnect_reported
        {
            self.disconnect_reported = true;
            return Poll::Ready(Some(Err(PacedBodyError::Disconnected)));
        }
        if self.offset >= self.data.len() {
            return Poll::Ready(None);
        }

        if let Some(deadline) = self.next_wait_deadline() {
            self.schedule_wait(deadline);
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        if Instant::now() < self.next_throttle_at {
            let deadline = self.next_throttle_at;
            self.schedule_wait(deadline);
            context.waker().wake_by_ref();
            return Poll::Pending;
        }

        let length = self.next_chunk_len();
        if length == 0 {
            self.disconnect_reported = true;
            return Poll::Ready(Some(Err(PacedBodyError::Disconnected)));
        }
        let end = self.offset + length;
        let data = self.data.slice(self.offset..end);
        self.offset = end;

        if let Some(profile) = self.schedule.throttle {
            let nanos = u128::from(length as u64)
                .saturating_mul(1_000_000_000)
                .checked_div(u128::from(profile.bytes_per_second.max(1)))
                .unwrap_or(0);
            self.next_throttle_at =
                Instant::now() + Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
        }
        Poll::Ready(Some(Ok(Frame::data(data))))
    }

    fn is_end_stream(&self) -> bool {
        self.is_complete()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.claimed_length)
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::*;
    use crate::traffic::{IntermittentProfile, JitterProfile, ThrottleProfile, TrafficSchedule};

    fn body_with_schedule(schedule: TrafficSchedule) -> PacedBody {
        PacedBody::new(
            Bytes::from_static(b"abcdefgh"),
            8,
            schedule,
            CancellationToken::new(),
        )
    }

    #[test]
    fn jitter_duration_stays_within_configured_inclusive_range() {
        let mut body = body_with_schedule(TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(10),
                maximum: Duration::from_millis(20),
                scope: JitterScope::PerChunk,
            }),
            seed: 42,
            ..TrafficSchedule::default()
        });

        for _ in 0..128 {
            assert!(
                (Duration::from_millis(10)..=Duration::from_millis(20))
                    .contains(&body.jitter_duration())
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn before_message_jitter_is_scheduled_only_once() {
        let mut body = body_with_schedule(TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(25),
                maximum: Duration::from_millis(25),
                scope: JitterScope::BeforeMessage,
            }),
            ..TrafficSchedule::default()
        });

        let first_deadline = body.next_wait_deadline().expect("initial jitter deadline");
        assert_eq!(
            first_deadline.duration_since(Instant::now()),
            Duration::from_millis(25)
        );

        tokio::time::advance(Duration::from_millis(25)).await;
        assert_eq!(body.next_wait_deadline(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn per_chunk_jitter_is_scheduled_once_for_each_offset() {
        let mut body = body_with_schedule(TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(25),
                maximum: Duration::from_millis(25),
                scope: JitterScope::PerChunk,
            }),
            ..TrafficSchedule::default()
        });

        assert!(body.next_wait_deadline().is_some());
        assert_eq!(body.next_wait_deadline(), None);

        body.offset = 1;
        assert!(body.next_wait_deadline().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn intermittent_schedule_allows_frames_during_available_window() {
        let mut body = body_with_schedule(TrafficSchedule {
            intermittent: Some(IntermittentProfile {
                available: Duration::from_millis(10),
                blocked: Duration::from_millis(20),
            }),
            ..TrafficSchedule::default()
        });

        tokio::time::advance(Duration::from_millis(9)).await;
        assert_eq!(body.next_wait_deadline(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn intermittent_schedule_waits_until_blocked_window_ends() {
        let mut body = body_with_schedule(TrafficSchedule {
            intermittent: Some(IntermittentProfile {
                available: Duration::from_millis(10),
                blocked: Duration::from_millis(20),
            }),
            ..TrafficSchedule::default()
        });

        tokio::time::advance(Duration::from_millis(10)).await;
        let deadline = body.next_wait_deadline().expect("blocked window deadline");

        assert_eq!(
            deadline.duration_since(Instant::now()),
            Duration::from_millis(20)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn throttle_splits_body_and_holds_later_chunks() {
        let mut body = PacedBody::new(
            Bytes::from_static(b"abcdefgh"),
            8,
            TrafficSchedule {
                throttle: Some(ThrottleProfile {
                    bytes_per_second: 4,
                    chunk_bytes: 4,
                }),
                ..TrafficSchedule::default()
            },
            CancellationToken::new(),
        );
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(first, Bytes::from_static(b"abcd"));
        let started = Instant::now();
        let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(second, Bytes::from_static(b"efgh"));
        assert!(Instant::now().duration_since(started) >= Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_emits_exact_prefix_then_error() {
        let mut body = PacedBody::new(
            Bytes::from_static(b"abcdefgh"),
            8,
            TrafficSchedule {
                disconnect_after_bytes: Some(3),
                ..TrafficSchedule::default()
            },
            CancellationToken::new(),
        );
        let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(prefix, Bytes::from_static(b"abc"));
        assert_eq!(
            body.frame().await.unwrap().unwrap_err(),
            PacedBodyError::Disconnected
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_interrupts_pending_delay() {
        let cancellation = CancellationToken::new();
        let mut body = PacedBody::new(
            Bytes::from_static(b"x"),
            1,
            TrafficSchedule {
                jitter: Some(super::super::JitterProfile {
                    minimum: Duration::from_mins(1),
                    maximum: Duration::from_mins(1),
                    scope: JitterScope::BeforeMessage,
                }),
                ..TrafficSchedule::default()
            },
            cancellation.clone(),
        );
        let task = tokio::spawn(async move { body.frame().await });
        tokio::task::yield_now().await;
        cancellation.cancel();
        let frame = task.await.unwrap().unwrap();
        assert_eq!(frame.unwrap_err(), PacedBodyError::Cancelled);
    }
}
