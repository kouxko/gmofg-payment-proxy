use std::sync::Arc;

use intercept_proxy_application::{
    BreakpointBodyCodecResolver, MessageContentKind, MessageContentViewModel,
};
use intercept_proxy_domain::BodyCodecKind;
use intercept_proxy_product_api::{BodyCodec, ProductError};
use intercept_proxy_runtime::Message;

use super::{RawBodyCodec, ShiftJisBodyCodec, Utf8BodyCodec};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpBodyMetadata {
    pub(crate) media_type: Option<String>,
    pub(crate) charset: Option<String>,
    pub(crate) content_kind: MessageContentKind,
}

pub(super) fn http_body_metadata(message: &Message) -> HttpBodyMetadata {
    let content_type = message
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(b"content-type"))
        .and_then(|header| std::str::from_utf8(&header.value).ok());
    parse_content_type(content_type)
}

pub(crate) fn resolve_message_codec(
    selected: BodyCodecKind,
    message: &Message,
) -> Arc<dyn BodyCodec> {
    match selected {
        BodyCodecKind::Auto => automatic_codec_for_metadata(&http_body_metadata(message)),
        BodyCodecKind::Raw => Arc::new(RawBodyCodec),
        BodyCodecKind::Utf8 => Arc::new(Utf8BodyCodec),
        BodyCodecKind::ShiftJis => Arc::new(ShiftJisBodyCodec),
    }
}

pub(crate) fn decode_message_body(
    message: &Message,
    body_codec: &dyn BodyCodec,
) -> (
    HttpBodyMetadata,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let metadata = http_body_metadata(message);
    let automatic = body_codec.id().starts_with("auto:");
    let declared_text = matches!(
        metadata.content_kind,
        MessageContentKind::Json | MessageContentKind::Xml | MessageContentKind::Text
    );
    if body_codec.id().ends_with("raw") || (automatic && !declared_text) {
        return (metadata, None, None, None);
    }
    let codec_id = Some(body_codec.id().to_owned());
    match body_codec.decode(&message.body) {
        Ok(text) => (metadata, Some(text), codec_id, None),
        Err(error) => (metadata, None, codec_id, Some(error.message)),
    }
}

#[derive(Debug, Default)]
pub struct HeaderBodyCodecResolver;

impl BreakpointBodyCodecResolver for HeaderBodyCodecResolver {
    fn resolve(&self, message: &MessageContentViewModel) -> Arc<dyn BodyCodec> {
        let content_type = message.headers.iter().find_map(|(name, values)| {
            name.eq_ignore_ascii_case("content-type")
                .then(|| values.first())
                .flatten()
                .map(String::as_str)
        });
        match message.codec_id.as_deref() {
            Some("utf-8" | "utf8") => Arc::new(Utf8BodyCodec),
            Some("shift-jis" | "shift_jis") => Arc::new(ShiftJisBodyCodec),
            Some("raw") => Arc::new(RawBodyCodec),
            _ => automatic_codec_for_metadata(&parse_content_type(content_type)),
        }
    }
}

fn parse_content_type(content_type: Option<&str>) -> HttpBodyMetadata {
    let Some(content_type) = content_type else {
        return unknown_body_metadata();
    };
    let mut segments = content_type.split(';');
    let media_type = segments
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let charset = segments.find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| normalize_charset(value))
    });
    let content_kind = media_type
        .as_deref()
        .map_or(MessageContentKind::Unknown, classify_media_type);
    HttpBodyMetadata {
        media_type,
        charset,
        content_kind,
    }
}

fn unknown_body_metadata() -> HttpBodyMetadata {
    HttpBodyMetadata {
        media_type: None,
        charset: None,
        content_kind: MessageContentKind::Unknown,
    }
}

fn automatic_codec_for_metadata(metadata: &HttpBodyMetadata) -> Arc<dyn BodyCodec> {
    let (id, inner): (&'static str, Arc<dyn BodyCodec>) = match metadata.charset.as_deref() {
        Some("utf-8" | "utf8") => ("auto:utf-8", Arc::new(Utf8BodyCodec)),
        Some("shift_jis" | "shift-jis" | "sjis" | "windows-31j" | "ms932" | "cp932") => {
            ("auto:shift-jis", Arc::new(ShiftJisBodyCodec))
        }
        Some(_) => ("auto:unsupported", Arc::new(UnsupportedCharsetBodyCodec)),
        None => match metadata.content_kind {
            MessageContentKind::Json | MessageContentKind::Xml => {
                ("auto:utf-8", Arc::new(Utf8BodyCodec))
            }
            MessageContentKind::Text => ("auto:missing", Arc::new(MissingCharsetBodyCodec)),
            MessageContentKind::Binary | MessageContentKind::Unknown => {
                ("auto:raw", Arc::new(RawBodyCodec))
            }
        },
    };
    Arc::new(AutomaticBodyCodec { id, inner })
}

struct AutomaticBodyCodec {
    id: &'static str,
    inner: Arc<dyn BodyCodec>,
}

impl std::fmt::Debug for AutomaticBodyCodec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomaticBodyCodec")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl BodyCodec for AutomaticBodyCodec {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        self.inner.decode(bytes)
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        self.inner.encode(text)
    }
}

fn classify_media_type(media_type: &str) -> MessageContentKind {
    let subtype = media_type.split_once('/').map(|(_, subtype)| subtype);
    if media_type == "application/json" || subtype.is_some_and(|value| value.ends_with("+json")) {
        MessageContentKind::Json
    } else if matches!(media_type, "application/xml" | "text/xml")
        || subtype.is_some_and(|value| value.ends_with("+xml"))
    {
        MessageContentKind::Xml
    } else if media_type.starts_with("text/") {
        MessageContentKind::Text
    } else if matches!(media_type, "application/octet-stream" | "application/pdf")
        || ["image/", "audio/", "video/"]
            .iter()
            .any(|prefix| media_type.starts_with(prefix))
    {
        MessageContentKind::Binary
    } else {
        MessageContentKind::Unknown
    }
}

fn normalize_charset(charset: &str) -> String {
    charset
        .trim()
        .trim_matches(['\'', '"'])
        .to_ascii_lowercase()
}

#[derive(Debug)]
struct UnsupportedCharsetBodyCodec;

impl BodyCodec for UnsupportedCharsetBodyCodec {
    fn id(&self) -> &'static str {
        "unsupported"
    }

    fn name(&self) -> &'static str {
        "Unsupported charset"
    }

    fn decode(&self, _bytes: &[u8]) -> Result<String, ProductError> {
        Err(unsupported_charset())
    }

    fn encode(&self, _text: &str) -> Result<Vec<u8>, ProductError> {
        Err(unsupported_charset())
    }
}

fn unsupported_charset() -> ProductError {
    ProductError::new(
        "BODY_CHARSET_UNSUPPORTED",
        "unsupported charset declared by Content-Type",
    )
}

#[derive(Debug)]
struct MissingCharsetBodyCodec;

impl BodyCodec for MissingCharsetBodyCodec {
    fn id(&self) -> &'static str {
        "missing"
    }

    fn name(&self) -> &'static str {
        "Missing charset"
    }

    fn decode(&self, _bytes: &[u8]) -> Result<String, ProductError> {
        Err(missing_charset())
    }

    fn encode(&self, _text: &str) -> Result<Vec<u8>, ProductError> {
        Err(missing_charset())
    }
}

fn missing_charset() -> ProductError {
    ProductError::new(
        "BODY_CHARSET_MISSING",
        "text Content-Type does not declare a charset",
    )
}
