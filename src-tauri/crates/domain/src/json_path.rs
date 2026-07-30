use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    name: String,
    indexes: Vec<usize>,
}

/// Parsed representation of the deliberately small `JSONPath` subset supported
/// by rule matching and mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath {
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonPathError {
    #[error("JSON path must start with `$.`")]
    MissingRoot,
    #[error("JSON path cannot be empty")]
    Empty,
    #[error("JSON path segment is invalid")]
    InvalidSegment,
    #[error("JSON path index is invalid")]
    InvalidIndex,
    #[error("JSON path does not exist")]
    NotFound,
    #[error("JSON path parent is not an object")]
    ParentNotObject,
    #[error("JSON path parent is not an array")]
    ParentNotArray,
    #[error("JSON array index is out of range")]
    IndexOutOfRange,
}

impl JsonPath {
    pub fn parse(path: &str) -> Result<Self, JsonPathError> {
        let Some(path) = path.strip_prefix("$.") else {
            return Err(JsonPathError::MissingRoot);
        };
        if path.is_empty() {
            return Err(JsonPathError::Empty);
        }
        path.split('.')
            .map(parse_segment)
            .collect::<Result<Vec<_>, _>>()
            .map(|segments| Self { segments })
    }

    #[must_use]
    pub fn resolve<'a>(&self, root: &'a Value) -> Option<&'a Value> {
        let mut current = root;
        for segment in &self.segments {
            current = current.get(&segment.name)?;
            for index in &segment.indexes {
                current = current.get(*index)?;
            }
        }
        Some(current)
    }

    pub fn set(&self, root: &mut Value, value: Value) -> Result<(), JsonPathError> {
        let mut current = root;
        for (segment_position, segment) in self.segments.iter().enumerate() {
            let is_last_segment = segment_position + 1 == self.segments.len();
            if is_last_segment && segment.indexes.is_empty() {
                let object = current
                    .as_object_mut()
                    .ok_or(JsonPathError::ParentNotObject)?;
                object.insert(segment.name.clone(), value);
                return Ok(());
            }

            current = current
                .get_mut(&segment.name)
                .ok_or(JsonPathError::NotFound)?;
            for (index_position, index) in segment.indexes.iter().enumerate() {
                let is_target = is_last_segment && index_position + 1 == segment.indexes.len();
                if is_target {
                    let array = current
                        .as_array_mut()
                        .ok_or(JsonPathError::ParentNotArray)?;
                    let slot = array
                        .get_mut(*index)
                        .ok_or(JsonPathError::IndexOutOfRange)?;
                    *slot = value;
                    return Ok(());
                }
                current = current
                    .get_mut(*index)
                    .ok_or(JsonPathError::IndexOutOfRange)?;
            }
        }
        Err(JsonPathError::Empty)
    }
}

fn parse_segment(segment: &str) -> Result<Segment, JsonPathError> {
    let name_end = segment.find('[').unwrap_or(segment.len());
    let name = &segment[..name_end];
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(JsonPathError::InvalidSegment);
    }

    let mut indexes = Vec::new();
    let mut rest = &segment[name_end..];
    while !rest.is_empty() {
        let Some(index_text) = rest.strip_prefix('[') else {
            return Err(JsonPathError::InvalidSegment);
        };
        let Some(close) = index_text.find(']') else {
            return Err(JsonPathError::InvalidIndex);
        };
        if index_text[..close].is_empty()
            || index_text[..close]
                .bytes()
                .any(|byte| !byte.is_ascii_digit())
        {
            return Err(JsonPathError::InvalidIndex);
        }
        indexes.push(
            index_text[..close]
                .parse()
                .map_err(|_| JsonPathError::InvalidIndex)?,
        );
        rest = &index_text[close + 1..];
    }
    Ok(Segment {
        name: name.to_owned(),
        indexes,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn one_parser_drives_resolve_and_set() {
        let path = JsonPath::parse("$.payment.items[0].result").expect("valid path");
        let mut value = json!({"payment": {"items": [{"result": "before"}]}});
        assert_eq!(path.resolve(&value), Some(&json!("before")));
        path.set(&mut value, json!("after")).expect("set");
        assert_eq!(path.resolve(&value), Some(&json!("after")));
    }

    #[test]
    fn invalid_paths_are_rejected_consistently() {
        for path in ["payment.result", "$.", "$.items[]", "$.items[-1]", "$.a..b"] {
            assert!(JsonPath::parse(path).is_err(), "{path}");
        }
    }
}
