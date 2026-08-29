// ISO 8583:1987 ASCII profile used by the built-in template.
// MTI and textual fields are ASCII. Bitmaps and security/ICC fields are binary.

fn field_specs() {
    [
        #{number: 2, name: "primary_account_number", kind: "ll_digits", length: 19},
        #{number: 3, name: "processing_code", kind: "fixed_digits", length: 6},
        #{number: 4, name: "amount", kind: "fixed_amount", length: 12},
        #{number: 5, name: "settlement_amount", kind: "fixed_digits", length: 12},
        #{number: 6, name: "cardholder_billing_amount", kind: "fixed_digits", length: 12},
        #{number: 7, name: "transmission_time", kind: "fixed_digits", length: 10},
        #{number: 8, name: "cardholder_billing_fee_amount", kind: "fixed_digits", length: 8},
        #{number: 9, name: "settlement_conversion_rate", kind: "fixed_digits", length: 8},
        #{number: 10, name: "cardholder_billing_conversion_rate", kind: "fixed_digits", length: 8},
        #{number: 11, name: "stan", kind: "fixed_digits", length: 6},
        #{number: 12, name: "local_transaction_time", kind: "fixed_digits", length: 6},
        #{number: 13, name: "local_transaction_date", kind: "fixed_digits", length: 4},
        #{number: 14, name: "expiration_date", kind: "fixed_digits", length: 4},
        #{number: 15, name: "settlement_date", kind: "fixed_digits", length: 4},
        #{number: 16, name: "currency_conversion_date", kind: "fixed_digits", length: 4},
        #{number: 17, name: "capture_date", kind: "fixed_digits", length: 4},
        #{number: 18, name: "merchant_type", kind: "fixed_digits", length: 4},
        #{number: 19, name: "acquiring_country_code", kind: "fixed_digits", length: 3},
        #{number: 20, name: "pan_extended_country_code", kind: "fixed_digits", length: 3},
        #{number: 21, name: "forwarding_country_code", kind: "fixed_digits", length: 3},
        #{number: 22, name: "pos_entry_mode", kind: "fixed_digits", length: 3},
        #{number: 23, name: "card_sequence_number", kind: "fixed_digits", length: 3},
        #{number: 24, name: "network_international_id", kind: "fixed_digits", length: 3},
        #{number: 25, name: "pos_condition_code", kind: "fixed_digits", length: 2},
        #{number: 26, name: "pos_capture_code", kind: "fixed_digits", length: 2},
        #{number: 27, name: "authorizing_id_response_length", kind: "fixed_digits", length: 1},
        #{number: 28, name: "transaction_fee_amount", kind: "fixed_ascii", length: 9},
        #{number: 29, name: "settlement_fee_amount", kind: "fixed_ascii", length: 9},
        #{number: 30, name: "transaction_processing_fee_amount", kind: "fixed_ascii", length: 9},
        #{number: 31, name: "settlement_processing_fee_amount", kind: "fixed_ascii", length: 9},
        #{number: 32, name: "acquiring_institution_id", kind: "ll_digits", length: 11},
        #{number: 33, name: "forwarding_institution_id", kind: "ll_digits", length: 11},
        #{number: 34, name: "pan_extended", kind: "ll_ascii", length: 28},
        #{number: 35, name: "track_2_data", kind: "ll_ascii", length: 37},
        #{number: 36, name: "track_3_data", kind: "lll_ascii", length: 104},
        #{number: 37, name: "retrieval_reference_number", kind: "fixed_ascii", length: 12},
        #{number: 38, name: "authorization_id_response", kind: "fixed_ascii", length: 6},
        #{number: 39, name: "response_code", kind: "fixed_ascii", length: 2},
        #{number: 40, name: "service_restriction_code", kind: "fixed_ascii", length: 3},
        #{number: 41, name: "terminal_id", kind: "fixed_ascii", length: 8},
        #{number: 42, name: "card_acceptor_id", kind: "fixed_ascii", length: 15},
        #{number: 43, name: "card_acceptor_name_location", kind: "fixed_ascii", length: 40},
        #{number: 44, name: "additional_response_data", kind: "ll_ascii", length: 25},
        #{number: 45, name: "track_1_data", kind: "ll_ascii", length: 76},
        #{number: 46, name: "additional_data_iso", kind: "lll_ascii", length: 999},
        #{number: 47, name: "additional_data_national", kind: "lll_ascii", length: 999},
        #{number: 48, name: "additional_data_private", kind: "lll_ascii", length: 999},
        #{number: 49, name: "currency", kind: "fixed_digits", length: 3},
        #{number: 50, name: "settlement_currency", kind: "fixed_digits", length: 3},
        #{number: 51, name: "cardholder_billing_currency", kind: "fixed_digits", length: 3},
        #{number: 52, name: "pin_data", kind: "fixed_blob", length: 8},
        #{number: 53, name: "security_control_information", kind: "fixed_digits", length: 16},
        #{number: 54, name: "additional_amounts", kind: "lll_ascii", length: 120},
        #{number: 55, name: "icc_data", kind: "lll_blob", length: 255},
        #{number: 56, name: "reserved_iso_56", kind: "lll_ascii", length: 999},
        #{number: 57, name: "reserved_national_57", kind: "lll_ascii", length: 999},
        #{number: 58, name: "reserved_national_58", kind: "lll_ascii", length: 999},
        #{number: 59, name: "reserved_national_59", kind: "lll_ascii", length: 999},
        #{number: 60, name: "reserved_private_60", kind: "lll_ascii", length: 999},
        #{number: 61, name: "reserved_private_61", kind: "lll_ascii", length: 999},
        #{number: 62, name: "reserved_private_62", kind: "lll_ascii", length: 999},
        #{number: 63, name: "reserved_private_63", kind: "lll_ascii", length: 999},
        #{number: 64, name: "message_authentication_code", kind: "fixed_blob", length: 8},
        #{number: 66, name: "settlement_code", kind: "fixed_digits", length: 1},
        #{number: 67, name: "extended_payment_code", kind: "fixed_digits", length: 2},
        #{number: 68, name: "receiving_country_code", kind: "fixed_digits", length: 3},
        #{number: 69, name: "settlement_country_code", kind: "fixed_digits", length: 3},
        #{number: 70, name: "network_management_code", kind: "fixed_digits", length: 3},
        #{number: 71, name: "message_number", kind: "fixed_digits", length: 4},
        #{number: 72, name: "last_message_number", kind: "fixed_digits", length: 4},
        #{number: 73, name: "action_date", kind: "fixed_digits", length: 6},
        #{number: 74, name: "credits_number", kind: "fixed_digits", length: 10},
        #{number: 75, name: "credits_reversal_number", kind: "fixed_digits", length: 10},
        #{number: 76, name: "debits_number", kind: "fixed_digits", length: 10},
        #{number: 77, name: "debits_reversal_number", kind: "fixed_digits", length: 10},
        #{number: 78, name: "transfer_number", kind: "fixed_digits", length: 10},
        #{number: 79, name: "transfer_reversal_number", kind: "fixed_digits", length: 10},
        #{number: 80, name: "inquiries_number", kind: "fixed_digits", length: 10},
        #{number: 81, name: "authorizations_number", kind: "fixed_digits", length: 10},
        #{number: 82, name: "credits_processing_fee_amount", kind: "fixed_digits", length: 12},
        #{number: 83, name: "credits_transaction_fee_amount", kind: "fixed_digits", length: 12},
        #{number: 84, name: "debits_processing_fee_amount", kind: "fixed_digits", length: 12},
        #{number: 85, name: "debits_transaction_fee_amount", kind: "fixed_digits", length: 12},
        #{number: 86, name: "credits_amount", kind: "fixed_digits", length: 16},
        #{number: 87, name: "credits_reversal_amount", kind: "fixed_digits", length: 16},
        #{number: 88, name: "debits_amount", kind: "fixed_digits", length: 16},
        #{number: 89, name: "debits_reversal_amount", kind: "fixed_digits", length: 16},
        #{number: 90, name: "original_data_elements", kind: "fixed_digits", length: 42},
        #{number: 91, name: "file_update_code", kind: "fixed_ascii", length: 1},
        #{number: 92, name: "file_security_code", kind: "fixed_ascii", length: 2},
        #{number: 93, name: "response_indicator", kind: "fixed_ascii", length: 5},
        #{number: 94, name: "service_indicator", kind: "fixed_ascii", length: 7},
        #{number: 95, name: "replacement_amounts", kind: "fixed_ascii", length: 42},
        #{number: 96, name: "message_security_code", kind: "fixed_blob", length: 8},
        #{number: 97, name: "net_settlement_amount", kind: "fixed_ascii", length: 17},
        #{number: 98, name: "payee", kind: "fixed_ascii", length: 25},
        #{number: 99, name: "settlement_institution_id", kind: "ll_digits", length: 11},
        #{number: 100, name: "receiving_institution_id", kind: "ll_digits", length: 11},
        #{number: 101, name: "file_name", kind: "ll_ascii", length: 17},
        #{number: 102, name: "account_id_1", kind: "ll_ascii", length: 28},
        #{number: 103, name: "account_id_2", kind: "ll_ascii", length: 28},
        #{number: 104, name: "transaction_description", kind: "lll_ascii", length: 100},
        #{number: 105, name: "reserved_iso_105", kind: "lll_ascii", length: 999},
        #{number: 106, name: "reserved_iso_106", kind: "lll_ascii", length: 999},
        #{number: 107, name: "reserved_iso_107", kind: "lll_ascii", length: 999},
        #{number: 108, name: "reserved_iso_108", kind: "lll_ascii", length: 999},
        #{number: 109, name: "reserved_iso_109", kind: "lll_ascii", length: 999},
        #{number: 110, name: "reserved_iso_110", kind: "lll_ascii", length: 999},
        #{number: 111, name: "reserved_iso_111", kind: "lll_ascii", length: 999},
        #{number: 112, name: "reserved_national_112", kind: "lll_ascii", length: 999},
        #{number: 113, name: "reserved_national_113", kind: "lll_ascii", length: 999},
        #{number: 114, name: "reserved_national_114", kind: "lll_ascii", length: 999},
        #{number: 115, name: "reserved_national_115", kind: "lll_ascii", length: 999},
        #{number: 116, name: "reserved_national_116", kind: "lll_ascii", length: 999},
        #{number: 117, name: "reserved_national_117", kind: "lll_ascii", length: 999},
        #{number: 118, name: "reserved_national_118", kind: "lll_ascii", length: 999},
        #{number: 119, name: "reserved_national_119", kind: "lll_ascii", length: 999},
        #{number: 120, name: "reserved_private_120", kind: "lll_ascii", length: 999},
        #{number: 121, name: "reserved_private_121", kind: "lll_ascii", length: 999},
        #{number: 122, name: "reserved_private_122", kind: "lll_ascii", length: 999},
        #{number: 123, name: "reserved_private_123", kind: "lll_ascii", length: 999},
        #{number: 124, name: "info_text", kind: "lll_ascii", length: 255},
        #{number: 125, name: "network_management_information", kind: "lll_ascii", length: 50},
        #{number: 126, name: "issuer_trace_id", kind: "lll_ascii", length: 6},
        #{number: 127, name: "reserved_private_127", kind: "lll_ascii", length: 999},
        #{number: 128, name: "message_authentication_code_2", kind: "fixed_blob", length: 8}
    ]
}

fn bitmap_has(bitmap, field_number) {
    let zero_based = field_number - 1;
    let byte_index = zero_based / 8;
    let bit_index = 7 - zero_based % 8;
    if byte_index >= bitmap.len() { return false; }
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
        if bytes[index].to_int() > 0x7f { throw field_name + " must contain ASCII bytes only"; }
    }
    bytes.as_string()
}

fn require_digit_text(text, field_name) {
    for character in text.chars() {
        if character < '0' || character > '9' { throw field_name + " must contain digits only"; }
    }
    text
}

fn read_bytes(message, offset, length, field_name) {
    if offset + length > message.len() { throw field_name + " exceeds the ISO 8583 message boundary"; }
    #{value: message.extract(offset, length), next_offset: offset + length}
}

fn read_ascii(message, offset, length, field_name) {
    let field = read_bytes(message, offset, length, field_name);
    field.value = ascii_from_blob(field.value, field_name);
    field
}

fn variable_length(message, offset, prefix_length, maximum, field_name) {
    let prefix = read_ascii(message, offset, prefix_length, field_name + " length");
    let length = require_digit_text(prefix.value, field_name + " length").parse_int();
    if length > maximum { throw field_name + " length exceeds profile maximum"; }
    #{length: length, next_offset: prefix.next_offset}
}

fn read_field(message, offset, spec) {
    let field_name = "DE" + spec.number + " " + spec.name;
    if spec.kind == "fixed_blob" { return read_bytes(message, offset, spec.length, field_name); }
    if spec.kind == "fixed_digits" || spec.kind == "fixed_amount" {
        let field = read_ascii(message, offset, spec.length, field_name);
        field.value = require_digit_text(field.value, field_name);
        return field;
    }
    if spec.kind == "fixed_ascii" { return read_ascii(message, offset, spec.length, field_name); }

    let prefix_length = if spec.kind.starts_with("ll_") { 2 } else { 3 };
    let declared = variable_length(message, offset, prefix_length, spec.length, field_name);
    if spec.kind.ends_with("_blob") {
        return read_bytes(message, declared.next_offset, declared.length, field_name);
    }
    let field = read_ascii(message, declared.next_offset, declared.length, field_name);
    if spec.kind.ends_with("_digits") { field.value = require_digit_text(field.value, field_name); }
    field
}

fn require_ascii(value, field_name) {
    let bytes = value.to_string().to_blob();
    ascii_from_blob(bytes, field_name);
    bytes
}

fn zero_pad(number, length, field_name) {
    if number < 0 { throw field_name + " cannot be negative"; }
    let text = number.to_string();
    if text.len() > length { throw field_name + " exceeds " + length + " digits"; }
    let result = "";
    for index in 0..(length - text.len()) { result += "0"; }
    result + text
}

fn byte_array_to_blob(value, field_name) {
    let result = blob(value.len(), 0);
    for index in 0..value.len() {
        let byte = value[index].to_int();
        if byte < 0 || byte > 255 { throw field_name + " contains a value outside one byte"; }
        result[index] = byte;
    }
    result
}

fn encode_field(value, spec) {
    let field_name = "DE" + spec.number + " " + spec.name;
    if spec.kind == "fixed_blob" {
        if value.len() != spec.length { throw field_name + " must contain exactly " + spec.length + " bytes"; }
        return byte_array_to_blob(value, field_name);
    }
    if spec.kind.ends_with("_blob") {
        if value.len() > spec.length { throw field_name + " length exceeds profile maximum"; }
        let prefix_length = if spec.kind.starts_with("ll_") { 2 } else { 3 };
        return zero_pad(value.len(), prefix_length, field_name + " length").to_blob()
            + byte_array_to_blob(value, field_name);
    }

    if spec.kind == "fixed_amount" { return zero_pad(value, spec.length, field_name).to_blob(); }
    let bytes = require_ascii(value, field_name);
    if spec.kind == "fixed_digits" {
        require_digit_text(value.to_string(), field_name);
        if bytes.len() != spec.length { throw field_name + " must contain exactly " + spec.length + " digits"; }
        return bytes;
    }
    if spec.kind == "fixed_ascii" {
        if bytes.len() != spec.length { throw field_name + " must contain exactly " + spec.length + " ASCII bytes"; }
        return bytes;
    }

    let payload = bytes;
    if spec.kind.ends_with("_digits") { require_digit_text(value.to_string(), field_name); }
    if payload.len() > spec.length { throw field_name + " length exceeds profile maximum"; }
    let prefix_length = if spec.kind.starts_with("ll_") { 2 } else { 3 };
    zero_pad(payload.len(), prefix_length, field_name + " length").to_blob() + payload
}

fn decode(message) {
    if message.len() < 12 { throw "ISO 8583 message must contain MTI and primary bitmap"; }
    let document = document::create();
    document.set("/message_type", require_digit_text(ascii_from_blob(message.extract(0, 4), "MTI"), "MTI"));

    let bitmap = message.extract(4, 8);
    let offset = 12;
    if bitmap_has(bitmap, 1) {
        if message.len() < 20 { throw "secondary bitmap indicator is set but bitmap is missing"; }
        bitmap += message.extract(12, 8);
        offset = 20;
        if bitmap_has(bitmap, 65) { throw "tertiary bitmap is outside this DE2-DE128 profile"; }
    }

    for spec in field_specs() {
        if bitmap_has(bitmap, spec.number) {
            let field = read_field(message, offset, spec);
            let path = "/" + spec.name;
            if spec.kind == "fixed_amount" { document.set(path, field.value.parse_int()); }
            else { document.set(path, field.value); }
            offset = field.next_offset;
        }
    }
    if offset != message.len() { throw "message has trailing bytes not described by the bitmap"; }
    document
}

fn encode(document) {
    if !document.has("/message_type") { throw "message_type is required"; }
    let message_type = require_digit_text(document.get("/message_type").to_string(), "message_type");
    if message_type.len() != 4 { throw "message_type must contain exactly 4 digits"; }

    let bitmap = blob(16, 0);
    let fields = blob();
    let secondary = false;
    for spec in field_specs() {
        let path = "/" + spec.name;
        if document.has(path) {
            bitmap = bitmap_set(bitmap, spec.number);
            fields += encode_field(document.get(path), spec);
            if spec.number > 64 { secondary = true; }
        }
    }
    if secondary { bitmap = bitmap_set(bitmap, 1); }

    let result = message_type.to_blob();
    result += bitmap.extract(0, 8);
    if secondary { result += bitmap.extract(8, 8); }
    result += fields;
    result
}
