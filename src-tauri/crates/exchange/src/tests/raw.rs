use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::super::*;

#[tokio::test]
async fn local_raw_server_echoes_each_written_chunk_without_protocol_processing() {
    let mut server = LocalRawServer::new();
    let connection = RawServer::connect(&mut server, b"first").await.unwrap();
    let (mut reader, mut writer) = connection.into_split();

    // 数据源是 TransparentExchange 向 LocalRawServer 写出的原始 App bytes；
    // 数据去向必须是同一 raw connection 的读取端，不得经过 Frame/Pipeline。
    writer.write(&[0x00, 0xff, 0x10]).await.unwrap();

    assert_eq!(reader.read().await.unwrap(), Some(vec![0x00, 0xff, 0x10]));
}

#[tokio::test]
async fn local_raw_writer_finish_propagates_eof_after_all_echoed_bytes() {
    let mut server = LocalRawServer::new();
    let connection = RawServer::connect(&mut server, b"first").await.unwrap();
    let (mut reader, mut writer) = connection.into_split();

    writer.write(b"reply-before-eof").await.unwrap();
    writer.finish().await.unwrap();

    assert_eq!(
        reader.read().await.unwrap(),
        Some(b"reply-before-eof".to_vec())
    );
    assert_eq!(reader.read().await.unwrap(), None);
}

#[tokio::test]
async fn local_raw_reader_drop_cancels_the_write_half() {
    let mut server = LocalRawServer::new();
    let connection = RawServer::connect(&mut server, b"first").await.unwrap();
    let (reader, mut writer) = connection.into_split();

    // Reader 是 local raw connection 唯一的输出消费者。释放它必须同步取消该连接，
    // 后续 write 直接失败；不存在后台 Echo task 会继续持有或吞掉 bytes。
    drop(reader);

    let error = writer.write(b"after-reader-drop").await.unwrap_err();
    assert_eq!(error.message, "LocalRawServer read half is closed");
}

#[tokio::test]
async fn local_raw_writer_drop_closes_the_read_half_without_a_background_task() {
    let mut server = LocalRawServer::new();
    let connection = RawServer::connect(&mut server, b"first").await.unwrap();
    let (mut reader, writer) = connection.into_split();

    // Drop 与显式 finish 具有相同的 channel 所有权语义：最后一个 Sender 被释放，
    // Reader 立即观察到 EOF，无需取消或 join 任何内部 task。
    drop(writer);

    let eof = tokio::time::timeout(std::time::Duration::from_secs(1), reader.read())
        .await
        .expect("reader must not wait for a leaked local Echo task")
        .unwrap();
    assert_eq!(eof, None);
}

#[tokio::test]
async fn transparent_exchange_connects_after_first_chunk_and_relays_both_directions() {
    let server_writes = Arc::new(Mutex::new(Vec::new()));
    let server_finishes = Arc::new(AtomicUsize::new(0));
    let app_writes = Arc::new(Mutex::new(Vec::new()));
    let app_finishes = Arc::new(AtomicUsize::new(0));
    let connects = Arc::new(Mutex::new(Vec::new()));

    let app = connection(
        queue_reader([Ok(Some(b"request".to_vec())), Ok(None)]),
        recording_writer(Arc::clone(&app_writes), Arc::clone(&app_finishes)),
    );
    let server_connection = connection(
        queue_reader([Ok(Some(b"response".to_vec())), Ok(None)]),
        recording_writer(Arc::clone(&server_writes), Arc::clone(&server_finishes)),
    );
    let server = FixedRawServer {
        connects: Arc::clone(&connects),
        connection: Some(server_connection),
    };

    TransparentExchange::new(app, Box::new(server))
        .exchange()
        .await
        .unwrap();

    assert_eq!(&*connects.lock(), &[b"request".to_vec()]);
    assert_eq!(&*server_writes.lock(), &[b"request".to_vec()]);
    assert_eq!(&*app_writes.lock(), &[b"response".to_vec()]);
    assert_eq!(server_finishes.load(Ordering::SeqCst), 1);
    assert_eq!(app_finishes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn transparent_exchange_does_not_connect_when_app_closes_without_data() {
    let connects = Arc::new(Mutex::new(Vec::new()));
    let app = connection(
        queue_reader([Ok(None)]),
        recording_writer(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicUsize::new(0)),
        ),
    );
    let server = FixedRawServer {
        connects: Arc::clone(&connects),
        connection: None,
    };

    TransparentExchange::new(app, Box::new(server))
        .exchange()
        .await
        .unwrap();

    assert!(connects.lock().is_empty());
}

#[tokio::test]
async fn transparent_relay_failure_cancels_peer_and_drops_all_connection_halves() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let app = connection(
        Box::new(TrackedReader {
            reads: VecDeque::from([
                Ok(Some(b"first".to_vec())),
                Err(Error::new("app read failed")),
            ]),
            pending: false,
            dropped: Arc::clone(&dropped),
        }),
        Box::new(TrackedWriter::new(Arc::clone(&dropped))),
    );
    let server_connection = connection(
        Box::new(TrackedReader {
            reads: VecDeque::new(),
            pending: true,
            dropped: Arc::clone(&dropped),
        }),
        Box::new(TrackedWriter::new(Arc::clone(&dropped))),
    );
    let server = FixedRawServer {
        connects: Arc::new(Mutex::new(Vec::new())),
        connection: Some(server_connection),
    };

    let error = TransparentExchange::new(app, Box::new(server))
        .exchange()
        .await
        .unwrap_err();

    assert_eq!(error.message, "app read failed");
    assert_eq!(dropped.load(Ordering::SeqCst), 4);
}

struct TestRawConnection {
    reader: Box<dyn RawReader>,
    writer: Box<dyn RawWriter>,
}

impl RawConnection for TestRawConnection {
    fn into_split(self: Box<Self>) -> (Box<dyn RawReader>, Box<dyn RawWriter>) {
        (self.reader, self.writer)
    }
}

fn connection(reader: Box<dyn RawReader>, writer: Box<dyn RawWriter>) -> Box<dyn RawConnection> {
    Box::new(TestRawConnection { reader, writer })
}

struct QueueRawReader {
    values: VecDeque<Result<Option<Vec<u8>>, Error>>,
}

#[async_trait]
impl RawReader for QueueRawReader {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
        self.values.pop_front().unwrap_or(Ok(None))
    }
}

fn queue_reader(
    values: impl IntoIterator<Item = Result<Option<Vec<u8>>, Error>>,
) -> Box<dyn RawReader> {
    Box::new(QueueRawReader {
        values: values.into_iter().collect(),
    })
}

struct RecordingRawWriter {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    finishes: Arc<AtomicUsize>,
}

#[async_trait]
impl RawWriter for RecordingRawWriter {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.writes.lock().push(bytes.to_vec());
        Ok(())
    }

    async fn finish(&mut self) -> Result<(), Error> {
        self.finishes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn recording_writer(
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    finishes: Arc<AtomicUsize>,
) -> Box<dyn RawWriter> {
    Box::new(RecordingRawWriter { writes, finishes })
}

struct FixedRawServer {
    connects: Arc<Mutex<Vec<Vec<u8>>>>,
    connection: Option<Box<dyn RawConnection>>,
}

#[async_trait]
impl RawServer for FixedRawServer {
    async fn connect(&mut self, first_app_bytes: &[u8]) -> Result<Box<dyn RawConnection>, Error> {
        self.connects.lock().push(first_app_bytes.to_vec());
        self.connection
            .take()
            .ok_or_else(|| Error::new("unexpected raw connect"))
    }
}

struct TrackedReader {
    reads: VecDeque<Result<Option<Vec<u8>>, Error>>,
    pending: bool,
    dropped: Arc<AtomicUsize>,
}

#[async_trait]
impl RawReader for TrackedReader {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if let Some(value) = self.reads.pop_front() {
            value
        } else if self.pending {
            std::future::pending().await
        } else {
            Ok(None)
        }
    }
}

impl Drop for TrackedReader {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

struct TrackedWriter {
    dropped: Arc<AtomicUsize>,
}

impl TrackedWriter {
    fn new(dropped: Arc<AtomicUsize>) -> Self {
        Self { dropped }
    }
}

#[async_trait]
impl RawWriter for TrackedWriter {
    async fn write(&mut self, _bytes: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    async fn finish(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

impl Drop for TrackedWriter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
