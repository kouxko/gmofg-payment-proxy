use super::*;

mod exchange;
mod pipeline;
mod raw;
mod support;

#[test]
fn protocol_markers_expose_the_expected_context_types() {
    fn assert_http(_: <Http as Protocol>::Context) {}
    fn assert_socket(_: <Socket as Protocol>::Context) {}

    assert_http(HttpContext {
        header: "POST /sale HTTP/1.1".to_owned(),
        body: "body".to_owned(),
        body_is_utf8: true,
        wire_body: b"body".to_vec(),
    });
    assert_socket(SocketContext {
        data: vec![0x01, 0x02],
    });
    assert_eq!(Upstream::KIND, DirectionKind::Upstream);
    assert_eq!(Downstream::KIND, DirectionKind::Downstream);
}
