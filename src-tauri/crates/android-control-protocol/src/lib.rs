//! Android Companion v2 wire contracts and length-prefixed framing.
//! This crate owns no application presentation, device state or transport resources.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

mod error;
pub use error::{AndroidControlError, AndroidControlResult};

pub const ANDROID_CONTROL_PROTOCOL_VERSION: u16 = 2;
pub const ANDROID_CONTROL_MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidControlTransport {
    LocalAbstractSocket,
    RescueActivity,
    AdbForceStop,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidNetworkState {
    Unknown,
    StartRequested,
    Running,
    StopRequested,
    Stopped,
    Faulted,
}

/// Android Companion 在控制协议中返回的机器状态。
///
/// 该结构不包含桌面展示文案或视觉语义；这些字段由 Rust 在协议校验完成后统一生成。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidCompanionStatus {
    pub serial: String,
    pub state: AndroidNetworkState,
    pub verified: bool,
    pub transport: AndroidControlTransport,
    pub active_profile_id: Option<String>,
    pub active_profile_fingerprint: Option<String>,
    pub active_route_fingerprint: Option<String>,
    pub active_route_count: usize,
    pub companion_process_running: Option<bool>,
    pub message: String,
    pub unsupported_fields: Vec<String>,
    pub stats: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AndroidControlRequest {
    pub version: u16,
    pub request_id: Uuid,
    pub operation: String,
    pub payload: Value,
}

impl AndroidControlRequest {
    pub fn new(operation: impl Into<String>, payload: Value) -> AndroidControlResult<Self> {
        let operation = operation.into();
        if !matches!(
            operation.as_str(),
            "profile_list"
                | "profile_get"
                | "profile_save"
                | "profile_delete"
                | "start"
                | "apply"
                | "apply_active"
                | "stop"
                | "emergency_restore"
                | "status"
                | "heartbeat"
        ) {
            return Err(AndroidControlError::InvalidOperation);
        }
        let request = Self {
            version: ANDROID_CONTROL_PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            operation,
            payload,
        };
        let encoded = serde_json::to_vec(&request).map_err(AndroidControlError::Encode)?;
        if encoded.len() > ANDROID_CONTROL_MAX_FRAME_BYTES {
            return Err(AndroidControlError::RequestTooLarge);
        }
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AndroidControlResponse {
    pub version: u16,
    pub request_id: Uuid,
    pub ok: bool,
    pub status: Option<AndroidCompanionStatus>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn encode_android_control_frame<T: Serialize>(value: &T) -> AndroidControlResult<Vec<u8>> {
    let payload = serde_json::to_vec(value).map_err(AndroidControlError::Encode)?;
    let length = u32::try_from(payload.len()).map_err(|_| AndroidControlError::LengthOverflow)?;
    if payload.len() > ANDROID_CONTROL_MAX_FRAME_BYTES {
        return Err(AndroidControlError::FrameTooLarge);
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_android_control_frame<T: for<'de> Deserialize<'de>>(
    frame: &[u8],
) -> AndroidControlResult<T> {
    if frame.len() < 4 {
        return Err(AndroidControlError::MissingLengthPrefix);
    }
    let declared = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if declared > ANDROID_CONTROL_MAX_FRAME_BYTES || declared != frame.len() - 4 {
        return Err(AndroidControlError::InvalidLength);
    }
    serde_json::from_slice(&frame[4..]).map_err(AndroidControlError::InvalidJson)
}

#[cfg(test)]
mod tests;
