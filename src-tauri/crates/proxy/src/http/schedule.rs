use super::{
    ErrorCode, FaultAction, IntermittentProfile, JitterProfile, ProxyError, Result,
    ThrottleProfile, TrafficDirection, TrafficSchedule,
};

pub(crate) fn traffic_schedule(
    actions: &[FaultAction],
    direction: TrafficDirection,
) -> Result<TrafficSchedule> {
    let mut schedule = TrafficSchedule::default();
    for action in actions {
        match action {
            FaultAction::Jitter {
                minimum,
                maximum,
                scope,
                seed,
            } => {
                schedule.jitter = Some(JitterProfile {
                    minimum: *minimum,
                    maximum: *maximum,
                    scope: *scope,
                });
                schedule.seed = *seed;
            }
            FaultAction::Throttle {
                bytes_per_second,
                chunk_bytes,
                direction: action_direction,
            } if *action_direction == direction => {
                if *bytes_per_second == 0 || *chunk_bytes == 0 {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "throttle rate and chunk size must be greater than zero",
                    ));
                }
                schedule.throttle = Some(ThrottleProfile {
                    bytes_per_second: *bytes_per_second,
                    chunk_bytes: *chunk_bytes,
                });
            }
            FaultAction::Intermittent {
                available,
                blocked,
                direction: action_direction,
            } if *action_direction == direction => {
                if available.is_zero() || blocked.is_zero() {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "intermittent windows must be greater than zero",
                    ));
                }
                schedule.intermittent = Some(IntermittentProfile {
                    available: *available,
                    blocked: *blocked,
                });
            }
            FaultAction::DisconnectDuringWrite {
                after_bytes,
                direction: action_direction,
            } if *action_direction == direction => {
                schedule.disconnect_after_bytes = Some(*after_bytes);
            }
            _ => {}
        }
    }
    Ok(schedule)
}
