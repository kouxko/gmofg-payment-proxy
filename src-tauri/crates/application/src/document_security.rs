//! 可移植 JSON 文档的字段名安全规则。
//!
//! 输入文档允许使用 snake_case、kebab-case 或 camelCase。安全判断先移除分隔符并
//! 统一小写，避免 `privateKey` 等未知字段绕过敏感信息检查后被 Serde 静默忽略。

/// 将 JSON 字段名规范化为只包含小写 ASCII 字母和数字的比较键。
pub(crate) fn canonical_field_name(field: &str) -> String {
    field
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

pub(crate) fn is_secret_field(field: &str) -> bool {
    matches!(
        canonical_field_name(field).as_str(),
        "password"
            | "passwordbytes"
            | "basicauthpassword"
            | "pkcs12password"
            | "privatekey"
            | "privatekeypem"
            | "privatekeyder"
            | "pkcs12"
            | "p12"
            | "secretvalue"
            | "protectedblob"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_common_json_naming_conventions() {
        for field in [
            "basic_auth_password",
            "basic-auth-password",
            "basicAuthPassword",
        ] {
            assert_eq!(canonical_field_name(field), "basicauthpassword");
            assert!(is_secret_field(field));
        }
        assert!(is_secret_field("privateKey"));
        assert!(is_secret_field("secret.value"));
    }
}
