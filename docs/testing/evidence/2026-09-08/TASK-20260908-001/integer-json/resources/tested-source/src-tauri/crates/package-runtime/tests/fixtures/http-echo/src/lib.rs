wit_bindgen::generate!({
    path: "../../../wit",
    world: "http-package",
});

const _: &str = include_str!("../../../../wit/protocol-package.wit");

struct HttpEcho;

impl Guest for HttpEcho {
    fn upstream_decode(input: String) -> Result<String, PackageError> {
        upstream_decode(input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: String) -> Result<String, PackageError> {
        Ok(input)
    }

    fn upstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        let _ = original_input;
        Ok(document_json)
    }

    fn downstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        let _ = original_input;
        Ok(document_json)
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Ok(document_json)
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        Ok(document_json)
    }
}

fn upstream_decode(input: String) -> Result<String, String> {
    if let Some(name) = input.strip_prefix("wasi-env:") {
        let present = std::env::var(name).is_ok_and(|value| !value.is_empty());
        return Ok(format!(r#"{{"present":{present}}}"#));
    }
    if let Some(path) = input.strip_prefix("wasi-read:") {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read host file through WASI: {error}"))?;
        return Ok(format!(r#"{{"contents":"{contents}"}}"#));
    }
    if let Some(url) = input.strip_prefix("websocket-roundtrip:") {
        use intercept_proxy::protocol_package::websocket::{Connection, Message};
        let connection = Connection::open(url)?;
        connection.send_text("guest-text")?;
        connection.send_binary(&[1, 2, 3])?;
        let text = match connection.receive()? {
            Message::Text(value) => value,
            other => return Err(format!("expected text message, received {other:?}")),
        };
        let binary = match connection.receive()? {
            Message::Binary(value) => value,
            other => return Err(format!("expected binary message, received {other:?}")),
        };
        let close_reason = match connection.receive()? {
            Message::Closed(reason) => reason,
            other => return Err(format!("expected close message, received {other:?}")),
        };
        return Ok(format!(
            r#"{{"text":"{text}","binary":{:?},"closed":"{}"}}"#,
            binary,
            close_reason.unwrap_or_default()
        ));
    }
    if let Some(url) = input.strip_prefix("websocket:") {
        let connection = intercept_proxy::protocol_package::websocket::Connection::open(url)?;
        connection.close()?;
        return Ok("{}".to_owned());
    }
    if let Some(authority) = input.strip_prefix("wasi-http:") {
        use wasip2::http::{
            outgoing_handler,
            types::{Fields, OutgoingRequest, Scheme},
        };
        let request = OutgoingRequest::new(Fields::new());
        request
            .set_scheme(Some(&Scheme::Http))
            .map_err(|()| "cannot set HTTP scheme".to_owned())?;
        request
            .set_authority(Some(authority))
            .map_err(|()| "cannot set HTTP authority".to_owned())?;
        request
            .set_path_with_query(Some("/health"))
            .map_err(|()| "cannot set HTTP path".to_owned())?;
        let response = outgoing_handler::handle(request, None)
            .map_err(|error| format!("HTTP request rejected: {error:?}"))?;
        response.subscribe().block();
        let response = response
            .get()
            .ok_or_else(|| "HTTP response future was not ready".to_owned())?
            .map_err(|()| "HTTP response was already consumed".to_owned())?
            .map_err(|error| format!("HTTP request failed: {error:?}"))?;
        return Ok(format!(r#"{{"status":{}}}"#, response.status()));
    }
    Ok(input)
}

fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
    move |message| PackageError {
        code: code.to_owned(),
        message,
    }
}

export!(HttpEcho);
