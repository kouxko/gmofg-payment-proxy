use super::{
    LegacySettingsChannelMapping, Map, PERSISTENCE_VERSION_FIELD, SETTINGS_PERSISTENCE_VERSION,
    SettingsDraft, Value,
};

pub(crate) fn serialize_settings(draft: &SettingsDraft) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(draft)?;
    value
        .as_object_mut()
        .expect("SettingsDraft always serializes as an object")
        .insert(
            PERSISTENCE_VERSION_FIELD.into(),
            Value::from(SETTINGS_PERSISTENCE_VERSION),
        );
    Ok(value)
}

pub(super) fn deserialize_settings(
    mut value: Value,
    defaults: &SettingsDraft,
    legacy_channels: &[LegacySettingsChannelMapping],
) -> Result<SettingsDraft, serde_json::Error> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| serde::de::Error::custom("settings root must be an object"))?;
    let version = take_persistence_version(object)?;
    let has_channels = object.contains_key("channels");
    match (version, has_channels) {
        (Some(SETTINGS_PERSISTENCE_VERSION) | None, true) => serde_json::from_value(value),
        (None, false) if !legacy_channels.is_empty() => {
            migrate_legacy_channel_settings(value, defaults, legacy_channels)
        }
        (Some(version), _) => Err(serde::de::Error::custom(format!(
            "unsupported settings persistence version {version}"
        ))),
        _ => serde_json::from_value(value),
    }
}

pub(super) fn take_persistence_version(
    object: &mut Map<String, Value>,
) -> Result<Option<u64>, serde_json::Error> {
    object
        .remove(PERSISTENCE_VERSION_FIELD)
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                serde::de::Error::custom("persistence version must be an unsigned integer")
            })
        })
        .transpose()
}

pub(super) fn migrate_legacy_channel_settings(
    mut value: Value,
    defaults: &SettingsDraft,
    mappings: &[LegacySettingsChannelMapping],
) -> Result<SettingsDraft, serde_json::Error> {
    let object = value
        .as_object_mut()
        .expect("settings root was validated before migration");
    let mut channels = Vec::with_capacity(mappings.len());
    for mapping in mappings {
        let expected = defaults
            .channels
            .iter()
            .find(|channel| channel.id.as_str() == mapping.channel_id)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "product profile does not declare legacy channel {}",
                    mapping.channel_id
                ))
            })?;
        channels.push(serde_json::json!({
            "id": expected.id,
            "display_name": expected.display_name,
            "enabled": take_legacy_field(object, mapping.enabled_field)?,
            "port": take_legacy_field(object, mapping.port_field)?,
            "upstream_url": take_legacy_field(object, mapping.upstream_url_field)?,
        }));
    }
    object.insert("channels".into(), Value::Array(channels));
    serde_json::from_value(value)
}

pub(super) fn take_legacy_field(
    object: &mut Map<String, Value>,
    field: &str,
) -> Result<Value, serde_json::Error> {
    object.remove(field).ok_or_else(|| {
        serde::de::Error::custom(format!("legacy settings field {field:?} is missing"))
    })
}
