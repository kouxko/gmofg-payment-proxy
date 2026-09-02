use std::{net::SocketAddr, time::Duration};

use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::mcp::protocol;

const EXCHANGE_DEADLINE: Duration = Duration::from_secs(10);

pub(super) async fn post_tool_call_to_non_loopback(
    address: SocketAddr,
    name: &str,
    body: Value,
) -> Value {
    assert!(!address.ip().is_loopback(), "target must be non-loopback");
    assert!(!address.ip().is_unspecified(), "target must be concrete");
    tokio::time::timeout(EXCHANGE_DEADLINE, exchange(address, name, body))
        .await
        .expect("complete non-loopback MCP HTTP exchange deadline")
}

async fn exchange(address: SocketAddr, name: &str, body: Value) -> Value {
    let socket = tokio::net::TcpSocket::new_v4().expect("create IPv4 MCP client socket");
    let source = SocketAddr::new(address.ip(), 0);
    socket
        .bind(source)
        .expect("bind non-loopback MCP client source");
    let mut stream = socket
        .connect(address)
        .await
        .expect("connect MCP interface");
    let local = stream.local_addr().expect("MCP client local address");
    let peer = stream.peer_addr().expect("MCP client peer address");
    assert!(!local.ip().is_loopback(), "source must be non-loopback");
    assert!(!local.ip().is_unspecified(), "source must be concrete");
    assert!(!peer.ip().is_loopback(), "peer must be non-loopback");
    assert_eq!(peer, address, "MCP client must reach the requested peer");
    let body = body.to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nOrigin: https://untrusted.example\r\nAuthorization: Bearer arbitrary-test-credential\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\nMcp-Method: tools/call\r\nMcp-Name: {name}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
        protocol::PROTOCOL_VERSION,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write non-loopback MCP request");

    let mut response = Vec::new();
    let header_end = read_headers(&mut stream, &mut response).await;
    let headers = std::str::from_utf8(&response[..header_end]).expect("ASCII HTTP headers");
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "non-loopback MCP response status: {headers:?}"
    );

    if let Some(length) = content_length(headers) {
        read_until(&mut stream, &mut response, header_end + length).await;
        return serde_json::from_slice(&response[header_end..header_end + length])
            .expect("JSON MCP response");
    }

    assert!(
        is_event_stream(headers),
        "unsupported HTTP framing: {headers:?}"
    );
    read_first_sse_data(&mut stream, &mut response, header_end).await
}

async fn read_headers(stream: &mut tokio::net::TcpStream, response: &mut Vec<u8>) -> usize {
    loop {
        if let Some(index) = response.windows(4).position(|part| part == b"\r\n\r\n") {
            return index + 4;
        }
        read_chunk(stream, response, "response headers").await;
    }
}

fn content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().expect("numeric Content-Length"))
        })
    })
}

fn is_event_stream(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("content-type")
                && value.trim().starts_with("text/event-stream")
        })
    })
}

async fn read_until(stream: &mut tokio::net::TcpStream, response: &mut Vec<u8>, target_len: usize) {
    while response.len() < target_len {
        read_chunk(stream, response, "Content-Length response body").await;
    }
}

async fn read_first_sse_data(
    stream: &mut tokio::net::TcpStream,
    response: &mut Vec<u8>,
    header_end: usize,
) -> Value {
    loop {
        let body = &response[header_end..];
        if let Some(event_end) = body
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .or_else(|| body.windows(2).position(|part| part == b"\n\n"))
        {
            let event = std::str::from_utf8(&body[..event_end]).expect("UTF-8 SSE event");
            let data = event
                .lines()
                .find_map(|line| line.strip_prefix("data:"))
                .expect("SSE data event")
                .trim();
            return serde_json::from_str(data).expect("JSON SSE data event");
        }
        read_chunk(stream, response, "SSE data event").await;
    }
}

async fn read_chunk(stream: &mut tokio::net::TcpStream, response: &mut Vec<u8>, frame: &str) {
    let mut chunk = [0_u8; 4096];
    let read = stream.read(&mut chunk).await.expect("read MCP response");
    assert_ne!(read, 0, "MCP connection closed before {frame}");
    response.extend_from_slice(&chunk[..read]);
}
