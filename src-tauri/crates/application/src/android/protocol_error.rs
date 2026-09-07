use crate::AppError;
use intercept_proxy_android_control_protocol::AndroidControlError;

impl From<AndroidControlError> for AppError {
    fn from(error: AndroidControlError) -> Self {
        match error {
            AndroidControlError::InvalidOperation => Self::new(
                "ANDROID_PROTOCOL_OPERATION_INVALID",
                "Android 控制操作不在协议白名单中。",
            ),
            AndroidControlError::Encode(error) => {
                Self::new("ANDROID_PROTOCOL_ENCODE_FAILED", error.to_string())
            }
            AndroidControlError::RequestTooLarge => Self::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "Android 控制请求超过 1 MiB 上限。",
            ),
            AndroidControlError::LengthOverflow => Self::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "Android 控制帧长度溢出。",
            ),
            AndroidControlError::FrameTooLarge => Self::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "Android 控制帧超过 1 MiB 上限。",
            ),
            AndroidControlError::MissingLengthPrefix => Self::new(
                "ANDROID_PROTOCOL_FRAME_INVALID",
                "Android 控制帧缺少长度前缀。",
            ),
            AndroidControlError::InvalidLength => {
                Self::new("ANDROID_PROTOCOL_FRAME_INVALID", "Android 控制帧长度无效。")
            }
            AndroidControlError::InvalidJson(error) => {
                Self::new("ANDROID_PROTOCOL_JSON_INVALID", error.to_string())
            }
        }
    }
}
