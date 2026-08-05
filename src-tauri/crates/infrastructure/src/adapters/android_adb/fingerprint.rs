//! 跨 Rust/Android 稳定的运行配置指纹。

use std::fmt::Write as _;

use intercept_proxy_application::{AppError, AppResult};
use serde::Serialize;

pub(super) fn sha256_json(value: &impl Serialize) -> AppResult<String> {
    let value = serde_json::to_value(value).map_err(|error| {
        AppError::new(
            "ANDROID_RUNTIME_FINGERPRINT_FAILED",
            format!("无法生成设备网络运行指纹：{error}"),
        )
    })?;
    let bytes = canonical_json(&value).into_bytes();
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let mut encoded = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

/// Android 的 JSON writer 会把 `/` 写成 `\/`，而 `serde_json` 保留 `/`。
/// 两端统一排序对象键并采用 JSON 标准字符串转义，避免合法 URL/CIDR 产生假冲突。
pub(super) fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => canonical_json_string(value),
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        serde_json::Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        format!("{}:{}", canonical_json_string(key), canonical_json(value))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn canonical_json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}
