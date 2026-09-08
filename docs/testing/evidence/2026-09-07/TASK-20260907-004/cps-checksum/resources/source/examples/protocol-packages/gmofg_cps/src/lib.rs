#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

mod codec;
mod finalize;
mod route;
mod signing;

wit_bindgen::generate!({
    path: "../../../src-tauri/crates/package-runtime/wit",
    world: "http-package-downstream-finalize",
});

const _: &str =
    include_str!("../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

struct Cps;

impl Guest for Cps {
    fn upstream_decode(input: String) -> Result<String, PackageError> {
        codec::decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: String) -> Result<String, PackageError> {
        codec::decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn upstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        codec::encode(&original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn downstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        codec::encode(&original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        codec::display(&document_json).map_err(package_error("DISPLAY_FAILED"))
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        codec::display(&document_json).map_err(package_error("DISPLAY_FAILED"))
    }

    fn downstream_finalize(http_json: String) -> Result<String, PackageError> {
        finalize::downstream(&http_json).map_err(package_error("CPS_FINALIZE_INVALID_INPUT"))
    }
}

fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
    move |message| PackageError {
        code: code.to_owned(),
        message,
    }
}

export!(Cps);

#[cfg(test)]
mod tests;
