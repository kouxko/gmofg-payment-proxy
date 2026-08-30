use regex::Regex;
use serde_json::Value;

use crate::JsonPath;

use super::{MatchContext, MatchField, MatchOperator};

pub(super) fn matches_condition(
    field: &MatchField,
    operator: &MatchOperator,
    context: &MatchContext<'_>,
) -> Result<bool, String> {
    let value = match field {
        MatchField::TerminalIp => context.terminal.source_ip.clone(),
        MatchField::CertificateFingerprint => context.terminal.certificate_sha256.clone(),
        MatchField::PathOrRequestType => {
            context.path_or_request_type.unwrap_or_default().to_owned()
        }
        MatchField::JsonPath(path) => {
            let Some(json) = context.json_body else {
                return Err("Body 不是可解析 JSON，JSON 字段条件不匹配".into());
            };
            let parsed = JsonPath::parse(path)
                .map_err(|_| "规则包含未通过保存校验的 JSON 字段路径".to_owned())?;
            let Some(value) = parsed.resolve(json) else {
                return Err(format!("JSON 字段路径不存在：{path}"));
            };
            json_scalar(value)
        }
    };
    Ok(match operator {
        MatchOperator::Equals(expected) => value == *expected,
        MatchOperator::Contains(fragment) => value.contains(fragment),
        MatchOperator::Regex(pattern) => Regex::new(pattern)
            .map_err(|_| "规则包含未通过保存校验的正则".to_owned())?
            .is_match(&value),
    })
}

fn json_scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}
