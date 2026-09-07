mod candidate;
mod capabilities;
mod outputs;

use serde_json::{Value, json};

#[rustfmt::skip]
fn status_input() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id"],"properties":{"candidate_id":{"type":"string"}}}) }
#[rustfmt::skip]
fn cancel_input() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id"],"properties":{"candidate_id":{"type":"string"}}}) }
#[rustfmt::skip]
fn apply_input() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id","confirmation_token"],"properties":{"candidate_id":{"type":"string"},"confirmation_token":{"type":"string"}}}) }

fn tool(input_schema: &Value, output_schema: &Value) -> Value {
    json!({"inputSchema": input_schema, "outputSchema": output_schema})
}

/// Builds the staged v1 JSON schemas independently from test fixtures and snapshots.
pub(super) fn document() -> Value {
    Value::Object(
        [
            ("origin".to_owned(), json!("revision16_manual_contract")),
            (
                "tools".to_owned(),
                Value::Object(
                    [
                        (
                            "mcp_environment_capabilities".to_owned(),
                            tool(
                                &capabilities::input_schema(),
                                &capabilities::output_schema(),
                            ),
                        ),
                        (
                            "environment_candidate_create".to_owned(),
                            tool(&candidate::input_schema(), &outputs::create_schema()),
                        ),
                        (
                            "environment_candidate_status".to_owned(),
                            tool(&status_input(), &outputs::status_schema()),
                        ),
                        (
                            "environment_candidate_cancel".to_owned(),
                            tool(&cancel_input(), &outputs::cancel_schema()),
                        ),
                        (
                            "environment_candidate_apply".to_owned(),
                            tool(&apply_input(), &outputs::apply_schema()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}
