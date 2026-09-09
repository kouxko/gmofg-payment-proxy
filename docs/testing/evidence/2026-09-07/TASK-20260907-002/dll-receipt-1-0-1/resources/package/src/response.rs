use serde::{Deserialize, Serialize};

use crate::{codec, model::DllDocument};

#[derive(Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum ResponseDocument {
    Dll(Box<DllDocument>),
    Receipt(ReceiptResponse),
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptResponse {
    #[serde(rename = "ResponseID")]
    response_id: String,
    #[serde(rename = "TransactionType")]
    transaction_type: String,
    #[serde(rename = "ResponseDateTime")]
    response_date_time: String,
    #[serde(rename = "ErrorCode")]
    error_code: String,
    #[serde(rename = "ErrorMessage")]
    error_message: String,
}

#[derive(Deserialize)]
struct ResponseKind {
    #[serde(rename = "TransactionType")]
    transaction_type: String,
}

fn is_receipt(input: &str) -> Result<bool, String> {
    // Read only the discriminator, then deserialize the original text with the
    // selected strict model so duplicate and unknown fields are still rejected.
    let kind: ResponseKind = serde_json::from_str(input)
        .map_err(|error| format!("invalid GMO-FG response TransactionType: {error}"))?;
    match kind.transaction_type.as_str() {
        "0000" | "0001" | "0002" => Ok(false),
        "U010" | "U011" => Ok(true),
        other => Err(format!(
            "unsupported GMO-FG Payment DLL TransactionType: {other}"
        )),
    }
}

fn receipt(input: &str) -> Result<ReceiptResponse, String> {
    serde_json::from_str(input).map_err(|error| format!("invalid GMO-FG receipt response: {error}"))
}

pub(crate) fn decode(input: &str) -> Result<ResponseDocument, String> {
    if is_receipt(input)? {
        receipt(input).map(ResponseDocument::Receipt)
    } else {
        codec::decode(input).map(|document| ResponseDocument::Dll(Box::new(document)))
    }
}

pub(crate) fn from_document(input: &str) -> Result<ResponseDocument, String> {
    if is_receipt(input)? {
        receipt(input).map(ResponseDocument::Receipt)
    } else {
        serde_json::from_str(input)
            .map(ResponseDocument::Dll)
            .map_err(|error| format!("GMO-FG Payment DLL Document is not valid: {error}"))
    }
}

pub(crate) fn encode(document: &ResponseDocument) -> Result<String, String> {
    match document {
        ResponseDocument::Dll(document) => codec::encode(document),
        ResponseDocument::Receipt(receipt) => serde_json::to_string(receipt)
            .map_err(|error| format!("cannot serialize GMO-FG receipt response: {error}")),
    }
}
