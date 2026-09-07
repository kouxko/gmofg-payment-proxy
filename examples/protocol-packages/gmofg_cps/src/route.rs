#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Pay,
    Confirm,
    Refund,
    Reverse,
}

const ROUTES: &[(Operation, &str)] = &[
    (Operation::Pay, "stub-products-pointPay"),
    (Operation::Pay, "products-pointPay"),
    (Operation::Pay, "stubPay"),
    (Operation::Pay, "pay"),
    (Operation::Confirm, "stub-products-pointConfirm"),
    (Operation::Confirm, "products-pointConfirm"),
    (Operation::Confirm, "stubConfirm"),
    (Operation::Confirm, "confirm"),
    (Operation::Refund, "stub-products-refund"),
    (Operation::Refund, "products-refund"),
    (Operation::Refund, "stubRefund"),
    (Operation::Refund, "refund"),
    (Operation::Reverse, "stub-products-reverse"),
    (Operation::Reverse, "products-reverse"),
    (Operation::Reverse, "stubReverse"),
    (Operation::Reverse, "reverse"),
];

pub(crate) fn operation_and_token(url: &str) -> Option<(Operation, String)> {
    let url = url.split(['?', '#']).next()?;
    let target = if let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    {
        let (_, path) = rest.split_once('/')?;
        format!("/{path}")
    } else if url.starts_with('/') {
        url.to_owned()
    } else {
        return None;
    };
    let path = target.as_str();
    for &(operation, endpoint) in ROUTES {
        let version = match operation {
            Operation::Pay | Operation::Confirm => "version=1.0/",
            Operation::Refund | Operation::Reverse => "",
        };
        let marker = format!("/gateway/{endpoint}/{version}B01/1/0/");
        // The configured CPS host can include a base path; the suffix is source-defined.
        if let Some((_, token)) = path.rsplit_once(&marker) {
            if token.is_empty() || token.contains('/') {
                return None;
            }
            return percent_decode(token).map(|token| (operation, token));
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let mut source = value.as_bytes().iter().copied();
    let mut bytes = Vec::with_capacity(value.len());
    while let Some(byte) = source.next() {
        if byte == b'%' {
            let high = char::from(source.next()?).to_digit(16)?;
            let low = char::from(source.next()?).to_digit(16)?;
            bytes.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            // '+' is literal in a URL path, not a form-encoded space.
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}
