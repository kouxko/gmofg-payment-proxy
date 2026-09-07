//! 弱网动作合并后的不可变调度描述。
//!
//! 调度只计算“何时、每块多大、何处断开”，不进行 I/O；无动作时保持零额外开销，非法
//! 参数应在更早的规则校验阶段拒绝。

use std::time::Duration;

const DEFAULT_TRAFFIC_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrafficDirection {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitterScope {
    BeforeMessage,
    PerChunk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitterProfile {
    pub minimum: Duration,
    pub maximum: Duration,
    pub scope: JitterScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThrottleProfile {
    pub bytes_per_second: u64,
    pub chunk_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntermittentProfile {
    pub available: Duration,
    pub blocked: Duration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrafficSchedule {
    pub jitter: Option<JitterProfile>,
    pub throttle: Option<ThrottleProfile>,
    pub intermittent: Option<IntermittentProfile>,
    pub disconnect_after_bytes: Option<usize>,
    pub seed: u64,
}

impl TrafficSchedule {
    #[must_use]
    pub fn is_passthrough(&self) -> bool {
        self.jitter.is_none()
            && self.throttle.is_none()
            && self.intermittent.is_none()
            && self.disconnect_after_bytes.is_none()
    }

    #[must_use]
    pub fn chunk_bytes(&self, body_len: usize) -> usize {
        if let Some(profile) = self.throttle {
            return profile.chunk_bytes.max(1);
        }
        let needs_default_chunking = self.intermittent.is_some()
            || self
                .jitter
                .is_some_and(|profile| profile.scope == JitterScope::PerChunk);
        if needs_default_chunking {
            return DEFAULT_TRAFFIC_CHUNK_BYTES.min(body_len.max(1));
        }
        body_len.max(1)
    }

    #[must_use]
    pub fn estimated_delay(&self, body_len: usize) -> Duration {
        let chunks = body_len
            .div_ceil(self.chunk_bytes(body_len))
            .max(1)
            .try_into()
            .unwrap_or(u32::MAX);
        let throttle = self.throttle.map_or(Duration::ZERO, |profile| {
            let nanos = (body_len as u128)
                .saturating_mul(1_000_000_000)
                .checked_div(u128::from(profile.bytes_per_second.max(1)))
                .unwrap_or(0);
            Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
        });
        let jitter = self.jitter.map_or(Duration::ZERO, |profile| {
            let count = if profile.scope == JitterScope::PerChunk {
                chunks
            } else {
                1
            };
            profile.maximum.saturating_mul(count)
        });
        let intermittent = self.intermittent.map_or(Duration::ZERO, |profile| {
            profile.blocked.saturating_mul(chunks)
        });
        throttle.saturating_add(jitter).saturating_add(intermittent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn before_message_jitter_contributes_one_maximum_delay() {
        let schedule = TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(10),
                maximum: Duration::from_millis(40),
                scope: JitterScope::BeforeMessage,
            }),
            ..TrafficSchedule::default()
        };

        assert_eq!(schedule.estimated_delay(16), Duration::from_millis(40));
    }

    #[test]
    fn per_chunk_jitter_contributes_one_maximum_delay_per_chunk() {
        let schedule = TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(10),
                maximum: Duration::from_millis(40),
                scope: JitterScope::PerChunk,
            }),
            throttle: Some(ThrottleProfile {
                bytes_per_second: u64::MAX,
                chunk_bytes: 4,
            }),
            ..TrafficSchedule::default()
        };

        assert_eq!(schedule.estimated_delay(9), Duration::from_millis(120));
    }

    #[test]
    fn per_chunk_actions_use_a_bounded_default_without_throttle() {
        let per_chunk_jitter = TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(10),
                maximum: Duration::from_millis(40),
                scope: JitterScope::PerChunk,
            }),
            ..TrafficSchedule::default()
        };
        let intermittent = TrafficSchedule {
            intermittent: Some(IntermittentProfile {
                available: Duration::from_millis(10),
                blocked: Duration::from_millis(20),
            }),
            ..TrafficSchedule::default()
        };
        let body_len = DEFAULT_TRAFFIC_CHUNK_BYTES * 2 + 1;

        assert_eq!(
            per_chunk_jitter.chunk_bytes(body_len),
            DEFAULT_TRAFFIC_CHUNK_BYTES
        );
        assert_eq!(
            per_chunk_jitter.estimated_delay(body_len),
            Duration::from_millis(120)
        );
        assert_eq!(
            intermittent.chunk_bytes(body_len),
            DEFAULT_TRAFFIC_CHUNK_BYTES
        );
    }

    #[test]
    fn estimated_delay_adds_throttle_jitter_and_blocked_windows() {
        let schedule = TrafficSchedule {
            jitter: Some(JitterProfile {
                minimum: Duration::from_millis(5),
                maximum: Duration::from_millis(10),
                scope: JitterScope::PerChunk,
            }),
            throttle: Some(ThrottleProfile {
                bytes_per_second: 10,
                chunk_bytes: 4,
            }),
            intermittent: Some(IntermittentProfile {
                available: Duration::from_millis(30),
                blocked: Duration::from_millis(20),
            }),
            ..TrafficSchedule::default()
        };

        assert_eq!(schedule.estimated_delay(8), Duration::from_millis(860));
    }
}
