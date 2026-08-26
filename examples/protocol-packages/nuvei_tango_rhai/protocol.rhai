fn framing_decision(reader, context) {
    let length_bytes = 4;
    let minimum_body_bytes = 14;
    let maximum_body_bytes = 1048572;

    if reader.available() < length_bytes {
        return framing::need_more(length_bytes);
    }

    let body_bytes = reader.peek_u32_be(0);
    if body_bytes < minimum_body_bytes {
        return framing::reject("Nuvei Tango length prefix is smaller than the minimum frame");
    }
    if body_bytes > maximum_body_bytes {
        return framing::reject("Nuvei Tango length prefix exceeds the 1 MiB package limit");
    }

    let frame_bytes = length_bytes + body_bytes;
    if reader.available() < frame_bytes {
        return framing::need_more(frame_bytes);
    }
    framing::complete(frame_bytes)
}

fn direction_context(context) {
    let value = blob(5, 0);
    value[0] = 0x4e;
    value[1] = 0x54;
    value[2] = 0x52;
    value[3] = 0x31;
    value[4] = if context.direction() == "upstream" { 0x55 } else { 0x44 };
    value
}

fn decode_message(origin, context) {
    let length_bytes = 4;
    let control_bytes = 4;
    let sequence_bytes = 8;
    let minimum_body_bytes = 14;
    let maximum_body_bytes = 1048572;

    if origin.len() < length_bytes {
        throw "Nuvei Tango frame is missing its length prefix";
    }

    let declared = (origin[0].to_int() << 24)
        | (origin[1].to_int() << 16)
        | (origin[2].to_int() << 8)
        | origin[3].to_int();
    if declared < minimum_body_bytes {
        throw "Nuvei Tango length prefix is smaller than the minimum frame";
    }
    if declared > maximum_body_bytes {
        throw "Nuvei Tango length prefix exceeds the 1 MiB package limit";
    }
    if origin.len() != length_bytes + declared {
        throw "Nuvei Tango length prefix does not match the complete frame";
    }

    let control = origin.extract(length_bytes, control_bytes);
    let sequence_blob = origin.extract(length_bytes + control_bytes, sequence_bytes);
    for index in 0..sequence_bytes {
        let byte = sequence_blob[index];
        if byte < 0x30 || byte > 0x39 {
            throw "Nuvei Tango sequence must contain exactly eight ASCII digits";
        }
    }
    let sequence = sequence_blob.as_string();

    let json_offset = length_bytes + control_bytes + sequence_bytes;
    let json_bytes = declared - control_bytes - sequence_bytes;
    let message = parse_json(origin.extract(json_offset, json_bytes).as_string());
    if message.len() != 1 {
        throw "Nuvei Tango JSON payload must contain one top-level message object";
    }
    let message_type = message.keys()[0];
    if message_type.len() == 0 {
        throw "Nuvei Tango message type must be a non-empty string";
    }

    let document = document::create();
    document.set("frame_length", declared);
    document.set("control_header", control);
    document.set("sequence", sequence);
    document.set("message_type", message_type);
    document.set("json_preview", message.to_json());
    document.set("encoding_context", direction_context(context));
    document
}

fn assert_read_only(document, expected) {
    for field in [
        "frame_length",
        "control_header",
        "sequence",
        "message_type",
        "json_preview",
        "encoding_context",
    ] {
        if !document.has(field) || document.get(field) != expected.get(field) {
            throw "Nuvei Tango read-only document was modified";
        }
    }
}

fn encode_message(origin, document, context) {
    let expected = decode_message(origin, context);
    assert_read_only(document, expected);
    origin
}

fn frame_upstream(reader, context) { framing_decision(reader, context) }
fn frame_downstream(reader, context) { framing_decision(reader, context) }

fn decode_upstream(origin, context) { decode_message(origin, context) }
fn decode_downstream(origin, context) { decode_message(origin, context) }

fn encode_upstream(origin, document, context) { encode_message(origin, document, context) }
fn encode_downstream(origin, document, context) { encode_message(origin, document, context) }
