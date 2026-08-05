use std::{collections::BTreeSet, net::IpAddr};

use crate::{
    ProductError, ProductLabels, ProductPersistenceMigrations, ProductProfile,
    ProductStorageNamespace, STANDARD_FAULT_CAPABILITY_IDS,
};

/// 在 Host 打开存储或启动后台任务前验证静态宿主契约。
pub fn validate_product_profile(product: &dyn ProductProfile) -> Result<(), ProductError> {
    let channel_ids = validate_channels(product)?;
    validate_storage(product.storage())?;
    validate_migrations(product.persistence_migrations(), &channel_ids)?;
    validate_fault_templates(product, &channel_ids)?;
    validate_labels(product.labels())
}

fn validate_channels(product: &dyn ProductProfile) -> Result<BTreeSet<&str>, ProductError> {
    let mut ids = BTreeSet::new();
    let mut enabled_ports = BTreeSet::new();
    for channel in product.channels() {
        validate_channel_id(channel.id)?;
        if !ids.insert(channel.id) {
            return invalid(format!("duplicate product channel ID {:?}", channel.id));
        }
        if channel.display_name.trim().is_empty() {
            return invalid(format!(
                "channel {:?} has an empty display name",
                channel.id
            ));
        }
        if channel.enabled_by_default
            && (channel.listen_port == 0 || !enabled_ports.insert(channel.listen_port))
        {
            return invalid(format!(
                "enabled channel {:?} has a zero or duplicate listen port",
                channel.id
            ));
        }
        if !valid_https_origin(channel.upstream_url) {
            return invalid(format!(
                "channel {:?} upstream must be an HTTPS origin without path, query, fragment, or userinfo",
                channel.id
            ));
        }
    }
    Ok(ids)
}

fn validate_fault_templates(
    product: &dyn ProductProfile,
    channel_ids: &BTreeSet<&str>,
) -> Result<(), ProductError> {
    let mut template_ids = BTreeSet::new();
    for template in product.fault_templates() {
        if template.id.is_empty() || !template_ids.insert(template.id) {
            return invalid(format!(
                "fault template ID {:?} is empty or duplicated",
                template.id
            ));
        }
        if !STANDARD_FAULT_CAPABILITY_IDS.contains(&template.id) {
            return invalid(format!(
                "fault template {:?} names an unknown capability",
                template.id
            ));
        }
        if !channel_ids.contains(template.default_channel_id) {
            return invalid(format!(
                "fault template {:?} references unknown channel {:?}",
                template.id, template.default_channel_id
            ));
        }
    }
    Ok(())
}

fn validate_migrations(
    migrations: ProductPersistenceMigrations,
    channel_ids: &BTreeSet<&str>,
) -> Result<(), ProductError> {
    let mut settings_fields = BTreeSet::new();
    for mapping in migrations.settings_channels {
        if !channel_ids.contains(mapping.channel_id) {
            return invalid(format!(
                "legacy settings mapping references unknown channel {:?}",
                mapping.channel_id
            ));
        }
        for field in [
            mapping.enabled_field,
            mapping.port_field,
            mapping.upstream_url_field,
        ] {
            if field.trim().is_empty() || !settings_fields.insert(field) {
                return invalid("legacy settings field names must be non-empty and unique");
            }
        }
    }

    let mut terminal_fields = BTreeSet::new();
    for field in migrations.terminal_body_fields {
        if field.trim().is_empty() || !terminal_fields.insert(*field) || *field == "body_bytes" {
            return invalid("legacy terminal body fields must be non-empty, unique aliases");
        }
    }
    Ok(())
}

fn validate_storage(storage: ProductStorageNamespace) -> Result<(), ProductError> {
    if [
        storage.database_file_name,
        storage.secret_service,
        storage.secret_account,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || storage.secret_aad.is_empty()
    {
        return invalid("product storage namespace must be non-empty");
    }
    if !valid_database_file_name(storage.database_file_name) {
        return invalid("product database file name must be one portable file-name component");
    }
    Ok(())
}

fn validate_labels(labels: ProductLabels) -> Result<(), ProductError> {
    if [
        labels.client_name,
        labels.upstream_name,
        labels.fault_rule_name_prefix,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return invalid("product labels must be non-empty");
    }
    Ok(())
}

fn validate_channel_id(value: &str) -> Result<(), ProductError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        invalid(format!("invalid product channel ID {value:?}"))
    }
}

fn valid_database_file_name(value: &str) -> bool {
    !matches!(value, "." | "..")
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b':' | 0))
}

/// 校验静态上游 origin。单个结尾 `/` 代表空路径。
pub(crate) fn valid_https_origin(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || !matches!(&rest[authority_end..], "" | "/")
    {
        return false;
    }

    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let suffix = &bracketed[end + 1..];
        return valid_optional_port(suffix)
            && bracketed[..end]
                .parse::<IpAddr>()
                .is_ok_and(|address| matches!(address, IpAddr::V6(_)));
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    valid_host(host)
}

fn valid_host(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }))
}

fn valid_optional_port(value: &str) -> bool {
    value.is_empty() || value.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ProductError> {
    Err(ProductError::new("PRODUCT_PROFILE_INVALID", message))
}
