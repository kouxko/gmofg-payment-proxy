const REDACTED_VALUE: &str = "[已脱敏]";
const REDACTED_PEM: &str = "[已脱敏 PEM]";
const REDACTED_BASE64: &str = "[已脱敏 Base64]";

pub(super) fn sanitize_optional(value: Option<&str>, max_chars: usize) -> Option<String> {
    value.map(|text| sanitize_text(text, max_chars))
}

pub(super) fn sanitize_text(value: &str, max_chars: usize) -> String {
    let value = redact_pem_blocks(value);
    let value = redact_secret_assignments(&value);
    let value = redact_long_base64(&value);
    truncate_chars(&value, max_chars)
}

fn redact_pem_blocks(value: &str) -> String {
    let mut output = Vec::new();
    let mut inside_pem = false;
    for line in value.lines() {
        if line.contains("-----BEGIN ") {
            if !inside_pem {
                output.push(REDACTED_PEM);
            }
            inside_pem = !line.contains("-----END ");
        } else if inside_pem {
            if line.contains("-----END ") {
                inside_pem = false;
            }
        } else {
            output.push(line);
        }
    }
    output.join("\n")
}

fn redact_secret_assignments(value: &str) -> String {
    const KEYS: [&str; 6] = [
        "pkcs12_password",
        "p12_password",
        "private_key_password",
        "password",
        "passwd",
        "pwd",
    ];
    let mut output = value.to_owned();
    let mut search_from = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        let Some((key_start, key)) = KEYS
            .iter()
            .filter_map(|key| {
                lower[search_from..]
                    .find(key)
                    .map(|at| (search_from + at, *key))
            })
            .min_by_key(|(at, _)| *at)
        else {
            break;
        };
        let key_end = key_start + key.len();
        let Some((value_start, value_end)) = assigned_value_range(&output, key_end) else {
            search_from = key_end;
            continue;
        };
        output.replace_range(value_start..value_end, REDACTED_VALUE);
        search_from = value_start + REDACTED_VALUE.len();
    }
    output
}

fn assigned_value_range(value: &str, key_end: usize) -> Option<(usize, usize)> {
    let bytes = value.as_bytes();
    let mut separator = key_end;
    while separator < bytes.len() && separator.saturating_sub(key_end) <= 16 {
        match bytes[separator] {
            b':' | b'=' => break,
            b' ' | b'\t' | b'\'' | b'"' => separator += 1,
            _ => return None,
        }
    }
    if separator >= bytes.len() || !matches!(bytes[separator], b':' | b'=') {
        return None;
    }
    let mut start = separator + 1;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(byte, b'\'' | b'"'));
    if quote.is_some() {
        start += 1;
    }
    let mut end = start;
    while end < bytes.len() {
        let byte = bytes[end];
        if quote.is_some_and(|expected| byte == expected)
            || (quote.is_none()
                && (byte.is_ascii_whitespace() || matches!(byte, b',' | b';' | b'&' | b'}' | b']')))
        {
            break;
        }
        end += 1;
    }
    (end > start).then_some((start, end))
}

fn redact_long_base64(value: &str) -> String {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, character) in value.char_indices() {
        let base64_character =
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '_' | '-');
        match (start, base64_character) {
            (None, true) => start = Some(index),
            (Some(run_start), false) => {
                if index.saturating_sub(run_start) >= 128 {
                    ranges.push((run_start, index));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(run_start) = start
        && value.len().saturating_sub(run_start) >= 128
    {
        ranges.push((run_start, value.len()));
    }
    let mut output = value.to_owned();
    for (start, end) in ranges.into_iter().rev() {
        output.replace_range(start..end, REDACTED_BASE64);
    }
    output
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
