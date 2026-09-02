use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
use serde_json::{Value, json};

const CORPUS: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/validation-corpus.json"
);

fn evaluate(cases: &[Value], validate: impl Fn(&str) -> bool) -> Vec<Value> {
    cases
        .iter()
        .map(|case| {
            let value = case["value"].as_str().expect("corpus value");
            json!({ "value": value, "valid": validate(value) })
        })
        .collect()
}

fn main() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("validation corpus");
    let result = json!({
        "id": evaluate(corpus["id"].as_array().expect("id corpus"), |value| {
            ProtocolPackageId::new(value).is_ok()
        }),
        "version": evaluate(corpus["version"].as_array().expect("version corpus"), |value| {
            ProtocolPackageVersion::new(value).is_ok()
        })
    });
    println!("{}", serde_json::to_string(&result).expect("result"));
}
