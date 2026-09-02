use super::{Map, PERSISTENCE_VERSION_FIELD, SETTINGS_PERSISTENCE_VERSION, SettingsDraft, Value};

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

pub(super) fn deserialize_settings(mut value: Value) -> Result<SettingsDraft, serde_json::Error> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| serde::de::Error::custom("settings root must be an object"))?;
    match take_persistence_version(object)? {
        Some(SETTINGS_PERSISTENCE_VERSION) => {}
        Some(version) => {
            return Err(serde::de::Error::custom(format!(
                "unsupported settings persistence version {version}"
            )));
        }
        None => {
            return Err(serde::de::Error::custom(
                "settings persistence version is required",
            ));
        }
    }
    let draft: SettingsDraft = serde_json::from_value(value.clone())?;
    if serde_json::to_value(&draft)? != value {
        return Err(serde::de::Error::custom(
            "settings contain unknown or non-current fields",
        ));
    }
    Ok(draft)
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
