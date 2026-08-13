// 入口函数由 manifest.toml 显式导出。
// 这里采用 2 字节大端长度头：长度只计算后面的 ISO 8583 报文。
// 这个长度头是 Socket 传输约定，不是 ISO 8583 标准本身的一部分。

import "libraries/iso8583" as iso8583;

const HEADER_LENGTH = 2;
const MAX_FRAME_LENGTH = 65535;

// 必需入口：从 TCP 字节流中找出一条完整 Frame。
// Reader 只读当前方向的缓冲区；函数不得保存 Reader。
// need_more(n) 的 n 是缓冲区需要达到的总长度，不是还要追加的长度。
// complete(n) 会把 FIFO 前 n 字节作为 origin，粘在后面的字节留给下一帧。
fn frame(reader, context) {
    if reader.available() < global::HEADER_LENGTH {
        return framing::need_more(global::HEADER_LENGTH);
    }

    let payload_length = reader.peek_u16_be(0);
    let frame_length = global::HEADER_LENGTH + payload_length;

    if frame_length > global::MAX_FRAME_LENGTH {
        return framing::reject("frame exceeds maximum length");
    }

    if reader.available() < frame_length {
        return framing::need_more(frame_length);
    }

    framing::complete(frame_length)
}

// 必需入口：origin 永远是一条完整 Frame，包括 2 字节长度头。
// 返回值必须是 document.toml 所声明的 Document。
// decode 抛错时宿主不会发送半成品 Frame，而是关闭当前连接并记录错误。
fn decode(origin, context) {
    if origin.len() < global::HEADER_LENGTH {
        throw "frame is shorter than its length header";
    }

    let declared_length = origin[0].to_int() * 256 + origin[1].to_int();
    if declared_length != origin.len() - global::HEADER_LENGTH {
        throw "frame length header does not match the received bytes";
    }

    let message = origin.extract(global::HEADER_LENGTH, declared_length);
    iso8583::decode(message)
}

// 可选入口：把规则处理后的 Document 编码为一条完整 Frame。
// 如果协议只需要观察、不需要重编码，可以直接写：return origin;
// 如果 manifest 没声明 encode，或 Listener 关闭该能力，宿主也会直接发送 origin。
// 返回值必须包含长度头在内的完整 Frame；失败时宿主关闭当前连接。
fn encode(origin, document, context) {
    let message = iso8583::encode(document);

    if message.len() > global::MAX_FRAME_LENGTH - global::HEADER_LENGTH {
        throw "encoded ISO 8583 message is too large";
    }

    let result = blob(global::HEADER_LENGTH, 0);
    result[0] = (message.len() >> 8) & 0xff;
    result[1] = message.len() & 0xff;
    result += message;
    result
}
