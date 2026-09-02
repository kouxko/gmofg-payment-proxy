use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{
    super::*,
    support::{QueueReader, RecordingWriter, document, text},
};

struct TextDecode;

#[async_trait]
impl<D: Direction> Decode<Http, D> for TextDecode {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        Ok(document(&context.body))
    }
}

#[async_trait]
impl<D: Direction> Decode<Socket, D> for TextDecode {
    async fn decode(&mut self, context: &SocketContext) -> Result<Document, Error> {
        let value = String::from_utf8(context.data.clone())
            .map_err(|error| Error::new(error.to_string()))?;
        Ok(document(&value))
    }
}

struct DocumentDisplay {
    fail: bool,
}

#[async_trait]
impl Display for DocumentDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        if self.fail {
            Err(Error::new("display failed"))
        } else {
            Ok(text(document))
        }
    }
}

struct SuffixRules {
    suffix: &'static str,
    fail: bool,
}

#[async_trait]
impl Rules for SuffixRules {
    async fn apply(&mut self, mut document: Document) -> Result<Document, Error> {
        if self.fail {
            return Err(Error::new("rules failed"));
        }
        let value = format!("{}{}", text(&document), self.suffix);
        document
            .set(
                &JsonPointer::property("value"),
                DocumentValue::String(value),
            )
            .unwrap();
        Ok(document)
    }
}

struct TextEncode;

#[async_trait]
impl<D: Direction> Encode<Http, D> for TextEncode {
    async fn encode(
        &mut self,
        original: &HttpContext,
        document: &Document,
    ) -> Result<HttpContext, Error> {
        Ok(HttpContext {
            header: original.header.clone(),
            body: text(document),
            body_is_utf8: true,
            wire_body: text(document).into_bytes(),
        })
    }
}

#[async_trait]
impl<D: Direction> Encode<Socket, D> for TextEncode {
    async fn encode(
        &mut self,
        _original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        Ok(SocketContext {
            data: text(document).into_bytes(),
        })
    }
}

struct LengthPrefixFrame;

#[async_trait]
impl<D: Direction> Frame<D> for LengthPrefixFrame {
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error> {
        let Some(length) = buffer.first().copied().map(usize::from) else {
            return Ok(FrameResult::NeedMore);
        };
        if buffer.len() < length + 1 {
            return Ok(FrameResult::NeedMore);
        }
        Ok(FrameResult::Complete {
            consumed: length + 1,
        })
    }
}

#[tokio::test]
async fn http_reader_fixes_display_and_writer_mutates_only_a_document_clone() {
    let original = HttpContext {
        header: "POST /sale HTTP/1.1".to_owned(),
        body: "sale".to_owned(),
        body_is_utf8: true,
        wire_body: b"sale".to_vec(),
    };
    let mut reader = QueueReader::<Http, Upstream>::contexts([original.clone()]);
    let mut pipeline = Pipeline::new(
        Box::new(HttpRead::new(
            Box::new(TextDecode),
            Box::new(DocumentDisplay { fail: false }),
        )),
        Box::new(Write::new(
            Box::new(SuffixRules {
                suffix: "-ruled",
                fail: false,
            }),
            Box::new(TextEncode),
        )),
    );
    let envelope = pipeline.read(&mut reader).await.unwrap().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut transport = RecordingWriter::<Http, Upstream>::new(Arc::clone(&recorded));

    let sent = pipeline.write(&mut transport, &envelope).await.unwrap();

    assert_eq!(envelope.context(), &original);
    assert_eq!(text(envelope.document()), "sale");
    assert_eq!(envelope.display(), "sale");
    assert_eq!(sent.body, "sale-ruled");
    assert_eq!(&*recorded.lock(), &[sent]);
}

#[tokio::test]
async fn display_failure_is_fail_open_with_protocol_specific_evidence() {
    let mut http_reader = QueueReader::<Http, Upstream>::contexts([HttpContext {
        header: "POST / HTTP/1.1".to_owned(),
        body: "plain body".to_owned(),
        body_is_utf8: true,
        wire_body: b"plain body".to_vec(),
    }]);
    let mut http = HttpRead::new(
        Box::new(TextDecode),
        Box::new(DocumentDisplay { fail: true }),
    );
    let http = ReadPipeline::read(&mut http, &mut http_reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(http.display(), "plain body");

    let mut socket_reader = QueueReader::<Socket, Upstream>::contexts([SocketContext {
        data: vec![1, b'a'],
    }]);
    let mut socket = SocketRead::new(
        Box::new(LengthPrefixFrame),
        Box::new(TextDecode),
        Box::new(DocumentDisplay { fail: true }),
    );
    let socket = ReadPipeline::read(&mut socket, &mut socket_reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(socket.display(), "01 61");
}

#[tokio::test]
async fn rules_failure_preserves_envelope_and_performs_no_transport_write() {
    let mut reader = QueueReader::<Http, Upstream>::contexts([HttpContext {
        header: String::new(),
        body: "sale".to_owned(),
        body_is_utf8: true,
        wire_body: b"sale".to_vec(),
    }]);
    let mut pipeline = Pipeline::new(
        Box::new(HttpRead::new(
            Box::new(TextDecode),
            Box::new(DocumentDisplay { fail: false }),
        )),
        Box::new(Write::new(
            Box::new(SuffixRules {
                suffix: "-ignored",
                fail: true,
            }),
            Box::new(TextEncode),
        )),
    );
    let envelope = pipeline.read(&mut reader).await.unwrap().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut transport = RecordingWriter::<Http, Upstream>::new(Arc::clone(&recorded));

    let error = pipeline.write(&mut transport, &envelope).await.unwrap_err();

    assert_eq!(error.message, "rules failed");
    assert_eq!(text(envelope.document()), "sale");
    assert!(recorded.lock().is_empty());
}

#[tokio::test]
async fn socket_reader_accumulates_until_exactly_one_frame_is_complete() {
    let mut reader = QueueReader::<Socket, Upstream>::contexts([
        SocketContext {
            data: vec![4, b's'],
        },
        SocketContext {
            data: b"ale".to_vec(),
        },
    ]);
    let mut read = SocketRead::new(
        Box::new(LengthPrefixFrame),
        Box::new(TextDecode),
        Box::new(DocumentDisplay { fail: false }),
    );

    let envelope = ReadPipeline::read(&mut read, &mut reader)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(envelope.context().data, vec![4, b's', b'a', b'l', b'e']);
    assert_eq!(text(envelope.document()), "\u{4}sale");
}

#[tokio::test]
async fn socket_reader_rejects_trailing_data_and_truncated_eof() {
    let mut trailing = QueueReader::<Socket, Upstream>::contexts([SocketContext {
        data: vec![1, b'a', 1, b'b'],
    }]);
    let mut read = SocketRead::new(
        Box::new(LengthPrefixFrame),
        Box::new(TextDecode),
        Box::new(DocumentDisplay { fail: false }),
    );
    assert_eq!(
        ReadPipeline::read(&mut read, &mut trailing)
            .await
            .unwrap_err()
            .message,
        "Socket read contained data beyond one complete Frame"
    );

    let mut truncated = QueueReader::<Socket, Upstream>::contexts([SocketContext {
        data: vec![4, b'a'],
    }]);
    assert_eq!(
        ReadPipeline::read(&mut read, &mut truncated)
            .await
            .unwrap_err()
            .message,
        "Socket closed before a complete Frame"
    );
}

#[tokio::test]
async fn writer_failure_is_a_single_error_without_partial_commit_model() {
    let mut reader = QueueReader::<Http, Upstream>::contexts([HttpContext {
        header: String::new(),
        body: "sale".to_owned(),
        body_is_utf8: true,
        wire_body: b"sale".to_vec(),
    }]);
    let mut pipeline = Pipeline::new(
        Box::new(HttpRead::new(
            Box::new(TextDecode),
            Box::new(DocumentDisplay { fail: false }),
        )),
        Box::new(Write::new(
            Box::new(SuffixRules {
                suffix: "",
                fail: false,
            }),
            Box::new(TextEncode),
        )),
    );
    let envelope = pipeline.read(&mut reader).await.unwrap().unwrap();
    let mut writer = RecordingWriter::<Http, Upstream>::new(Arc::new(Mutex::new(Vec::new())));
    writer.failure = Some(Error::new("write failed"));

    let error = pipeline.write(&mut writer, &envelope).await.unwrap_err();

    assert_eq!(error, Error::new("write failed"));
}
