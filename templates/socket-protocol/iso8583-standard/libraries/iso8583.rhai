// 一个可运行方向明确的 ISO 8583 ASCII 起始模板。
//
// 支持：ASCII MTI、8 字节二进制主位图，以及 DE3/4/7/11/41/49。
// 生产协议若包含变长字段、第二位图或厂商私有域，应在这里扩展。

fn bitmap_has(bitmap, field_number) {
    let zero_based = field_number - 1;
    let byte_index = zero_based / 8;
    let bit_index = 7 - zero_based % 8;
    (bitmap[byte_index].to_int() & (1 << bit_index)) != 0
}

fn bitmap_set(bitmap, field_number) {
    let zero_based = field_number - 1;
    let byte_index = zero_based / 8;
    let bit_index = 7 - zero_based % 8;
    bitmap[byte_index] = bitmap[byte_index].to_int() | (1 << bit_index);
    bitmap
}

fn ascii_from_blob(bytes, field_name) {
    for index in 0..bytes.len() {
        if bytes[index].to_int() > 0x7f {
            throw field_name + " must contain ASCII bytes only";
        }
    }
    bytes.as_string()
}

fn require_digit_text(text, field_name) {
    for character in text.chars() {
        if character < '0' || character > '9' {
            throw field_name + " must contain digits only";
        }
    }
    text
}

fn read_ascii(message, offset, length, field_name) {
    if offset + length > message.len() {
        throw field_name + " exceeds the ISO 8583 message boundary";
    }

    let value = ascii_from_blob(message.extract(offset, length), field_name);
    #{ value: value, next_offset: offset + length }
}

fn read_digits(message, offset, length, field_name) {
    let field = read_ascii(message, offset, length, field_name);
    field.value = require_digit_text(field.value, field_name);
    field
}

fn require_fixed_ascii(value, length, field_name) {
    let text = value.to_string();
    let bytes = text.to_blob();
    if bytes.len() != length {
        throw field_name + " must contain exactly " + length + " ASCII bytes";
    }

    for index in 0..bytes.len() {
        if bytes[index].to_int() > 0x7f {
            throw field_name + " must contain ASCII bytes only";
        }
    }

    bytes
}

fn require_digits(value, length, field_name) {
    let text = value.to_string();
    if text.len() != length {
        throw field_name + " must contain exactly " + length + " digits";
    }

    require_digit_text(text, field_name).to_blob()
}

fn zero_pad(number, length, field_name) {
    if number < 0 {
        throw field_name + " cannot be negative";
    }

    let text = number.to_string();
    if text.len() > length {
        throw field_name + " exceeds " + length + " digits";
    }

    let result = "";
    for index in 0..(length - text.len()) {
        result += "0";
    }
    result + text
}

fn decode(message) {
    if message.len() < 12 {
        throw "ISO 8583 message must contain MTI and primary bitmap";
    }

    let document = document::create();
    document.set(
        "message_type",
        require_digit_text(ascii_from_blob(message.extract(0, 4), "MTI"), "MTI")
    );

    let bitmap = message.extract(4, 8);
    if bitmap_has(bitmap, 1) {
        throw "secondary bitmap is not supported by this starter template";
    }

    let offset = 12;

    if bitmap_has(bitmap, 3) {
        let field = read_digits(message, offset, 6, "DE3 processing_code");
        document.set("processing_code", field.value);
        offset = field.next_offset;
    }

    if bitmap_has(bitmap, 4) {
        let field = read_digits(message, offset, 12, "DE4 amount");
        document.set("amount", field.value.parse_int());
        offset = field.next_offset;
    }

    if bitmap_has(bitmap, 7) {
        let field = read_digits(message, offset, 10, "DE7 transmission_time");
        document.set("transmission_time", field.value);
        offset = field.next_offset;
    }

    if bitmap_has(bitmap, 11) {
        let field = read_digits(message, offset, 6, "DE11 stan");
        document.set("stan", field.value);
        offset = field.next_offset;
    }

    if bitmap_has(bitmap, 41) {
        let field = read_ascii(message, offset, 8, "DE41 terminal_id");
        document.set("terminal_id", field.value);
        offset = field.next_offset;
    }

    if bitmap_has(bitmap, 49) {
        let field = read_digits(message, offset, 3, "DE49 currency");
        document.set("currency", field.value);
        offset = field.next_offset;
    }

    if offset != message.len() {
        throw "message contains fields not implemented by this starter template";
    }

    document
}

fn encode(document) {
    if !document.has("message_type") {
        throw "message_type is required";
    }

    let message_type = require_digits(document.get("message_type"), 4, "message_type");
    let bitmap = blob(8, 0);
    let fields = blob();

    if document.has("processing_code") {
        bitmap = bitmap_set(bitmap, 3);
        fields += require_digits(document.get("processing_code"), 6, "processing_code");
    }

    if document.has("amount") {
        bitmap = bitmap_set(bitmap, 4);
        fields += zero_pad(document.get("amount"), 12, "amount").to_blob();
    }

    if document.has("transmission_time") {
        bitmap = bitmap_set(bitmap, 7);
        fields += require_digits(document.get("transmission_time"), 10, "transmission_time");
    }

    if document.has("stan") {
        bitmap = bitmap_set(bitmap, 11);
        fields += require_digits(document.get("stan"), 6, "stan");
    }

    if document.has("terminal_id") {
        bitmap = bitmap_set(bitmap, 41);
        fields += require_fixed_ascii(document.get("terminal_id"), 8, "terminal_id");
    }

    if document.has("currency") {
        bitmap = bitmap_set(bitmap, 49);
        fields += require_digits(document.get("currency"), 3, "currency");
    }

    let result = message_type;
    result += bitmap;
    result += fields;
    result
}
