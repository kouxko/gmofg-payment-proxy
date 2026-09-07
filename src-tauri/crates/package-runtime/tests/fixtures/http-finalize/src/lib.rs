#[cfg(feature = "invalid-signature")]
wit_bindgen::generate!({
    path: "wit-invalid",
    world: "http-package-invalid-finalize",
});

#[cfg(all(
    not(feature = "invalid-signature"),
    feature = "upstream",
    feature = "downstream"
))]
wit_bindgen::generate!({
    path: "../../../wit",
    world: "http-package-finalize",
});

#[cfg(all(
    not(feature = "invalid-signature"),
    feature = "upstream",
    not(feature = "downstream")
))]
wit_bindgen::generate!({
    path: "../../../wit",
    world: "http-package-upstream-finalize",
});

#[cfg(all(
    not(feature = "invalid-signature"),
    not(feature = "upstream"),
    feature = "downstream"
))]
wit_bindgen::generate!({
    path: "../../../wit",
    world: "http-package-downstream-finalize",
});

#[cfg(all(
    not(feature = "invalid-signature"),
    not(feature = "upstream"),
    not(feature = "downstream")
))]
wit_bindgen::generate!({
    path: "../../../wit",
    world: "http-package",
});

struct HttpFinalize;

impl Guest for HttpFinalize {
    fn upstream_decode(input: String) -> Result<String, PackageError> {
        Ok(input)
    }

    fn downstream_decode(input: String) -> Result<String, PackageError> {
        Ok(input)
    }

    fn upstream_encode(
        _original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        Ok(document_json)
    }

    fn downstream_encode(
        _original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        Ok(document_json)
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Ok(document_json)
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        Ok(document_json)
    }

    #[cfg(all(feature = "upstream", not(feature = "invalid-signature")))]
    fn upstream_finalize(http_json: String) -> Result<String, PackageError> {
        finalize(http_json, "upstream")
    }

    #[cfg(all(feature = "downstream", not(feature = "invalid-signature")))]
    fn downstream_finalize(http_json: String) -> Result<String, PackageError> {
        finalize(http_json, "downstream")
    }

    #[cfg(feature = "invalid-signature")]
    fn downstream_finalize(http_json: u32) -> u32 {
        http_json
    }
}

#[cfg(any(feature = "upstream", feature = "downstream"))]
fn finalize(http_json: String, direction: &str) -> Result<String, PackageError> {
    if http_json == "guest-error" {
        return Err(PackageError {
            code: "BODY_ENCODE_FAILED".to_owned(),
            message: format!("{direction} finalize rejected the HTTP JSON"),
        });
    }
    if http_json == "trap" {
        panic!("{direction} finalize trap");
    }
    let code = match direction {
        "upstream" => "201",
        "downstream" => "202",
        _ => unreachable!(),
    };
    let finalized = match direction {
        "upstream" => http_json.replace("before", direction),
        "downstream" if http_json == "before" => "downstream".to_owned(),
        "downstream" => http_json
            .replace("\"X-Value\":\"before\"", "\"X-Value\":\"downstream\"")
            .replace("\"body\":\"before\"", "\"body\":\"downstream\"")
            .replace(
                "\"body\":\"\\\"before\\\"\"",
                "\"body\":\"\\\"downstream\\\"\"",
            ),
        _ => unreachable!(),
    };
    Ok(finalized.replace("\"code\":200", &format!("\"code\":{code}")))
}

export!(HttpFinalize);
