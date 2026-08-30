//! Local Boa-backed package execution for the generic Sidecar.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    future::ready,
    path::{Path, PathBuf},
    rc::Rc,
};

use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, Module, Source,
    builtins::promise::PromiseState,
    js_string,
    module::{ModuleLoader, ModuleRequest, Referrer, resolve_module_specifier},
    object::{JsObject, builtins::JsUint8Array},
};
use intercept_proxy_domain::{Document, DomainError, ErrorCode};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
    PackageKind,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::PackageArchive;

const SOCKET_PROTOCOL_EXPORTS: &[(&str, FixedExport)] = &[
    ("upstreamFrame", FixedExport::Protocol),
    ("downstreamFrame", FixedExport::Protocol),
    ("upstreamDecode", FixedExport::Protocol),
    ("downstreamDecode", FixedExport::Protocol),
    ("upstreamEncode", FixedExport::Protocol),
    ("downstreamEncode", FixedExport::Protocol),
];
const HTTP_PROTOCOL_EXPORTS: &[(&str, FixedExport)] = &[
    ("upstreamDecode", FixedExport::Protocol),
    ("downstreamDecode", FixedExport::Protocol),
    ("upstreamEncode", FixedExport::Protocol),
    ("downstreamEncode", FixedExport::Protocol),
];
const DISPLAY_EXPORTS: &[(&str, FixedExport)] = &[
    ("upstreamDisplay", FixedExport::Display),
    ("downstreamDisplay", FixedExport::Display),
];

#[derive(Clone, Copy)]
enum FixedExport {
    Protocol,
    Display,
}

const MODULE_ROOT: &str = "/package";

#[derive(Debug, Default)]
struct PackageModuleLoader {
    modules: RefCell<BTreeMap<PathBuf, Module>>,
}

impl PackageModuleLoader {
    fn insert(&self, path: &str, module: Module) {
        self.modules
            .borrow_mut()
            .insert(Path::new(MODULE_ROOT).join(path), module);
    }
}

impl ModuleLoader for PackageModuleLoader {
    fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        request: ModuleRequest,
        context: &RefCell<&mut Context>,
    ) -> impl Future<Output = JsResult<Module>> {
        let result = (|| {
            let specifier = request.specifier().to_std_string_escaped();
            if !specifier.starts_with("./") && !specifier.starts_with("../") {
                return Err(JsNativeError::typ()
                    .with_message("package module imports must use relative specifiers")
                    .into());
            }
            let path = resolve_module_specifier(
                Some(Path::new(MODULE_ROOT)),
                request.specifier(),
                referrer.path(),
                &mut context.borrow_mut(),
            )?;
            self.modules.borrow().get(&path).cloned().ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("package-relative module could not be found")
                    .into()
            })
        })();
        ready(result)
    }
}

/// Local Sidecar JavaScript execution error.
#[derive(Clone)]
pub struct LocalSidecarError {
    message: String,
}

impl LocalSidecarError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the stable package error code.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        ErrorCode::ProtocolPackageInvalid
    }

    /// Converts this Sidecar error to the package-invalid Domain error used by callers.
    #[must_use]
    pub fn into_domain_error(self) -> DomainError {
        DomainError::new(self.code(), self.message)
    }
}

impl fmt::Debug for LocalSidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSidecarError")
            .field("code", &self.code())
            .finish()
    }
}

impl fmt::Display for LocalSidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LocalSidecarError {}

impl From<JsError> for LocalSidecarError {
    fn from(value: JsError) -> Self {
        Self::new(format!("JavaScript execution failed: {value}"))
    }
}

/// One loaded local package runtime. Its Boa context and evaluated module exports are reused.
pub struct LocalSidecarRuntime {
    kind: PackageKind,
    context: Context,
    protocol: Module,
    display: Module,
    exports: BTreeMap<&'static str, JsObject>,
}

impl fmt::Debug for LocalSidecarRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSidecarRuntime")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl LocalSidecarRuntime {
    /// Loads, links, evaluates and validates one package archive.
    pub fn load(archive: &PackageArchive) -> Result<Self, LocalSidecarError> {
        let loader = Rc::new(PackageModuleLoader::default());
        let mut context = Context::builder()
            .module_loader(Rc::clone(&loader))
            .build()
            .map_err(|error| LocalSidecarError::new(error.to_string()))?;
        let mut protocol = None;
        let mut display = None;
        for (path, bytes) in archive.files() {
            if path == "manifest.json" {
                continue;
            }
            let module_path = Path::new(MODULE_ROOT).join(path);
            let source = Source::from_bytes(bytes).with_path(&module_path);
            let module = Module::parse(source, None, &mut context)?;
            loader.insert(path, module.clone());
            match path {
                "protocol.js" => protocol = Some(module),
                "display.js" => display = Some(module),
                _ => {}
            }
        }
        let mut runtime = Self {
            kind: archive.manifest().kind(),
            context,
            protocol: protocol.ok_or_else(|| LocalSidecarError::new("missing protocol.js"))?,
            display: display.ok_or_else(|| LocalSidecarError::new("missing display.js"))?,
            exports: BTreeMap::new(),
        };
        runtime.evaluate(FixedExport::Protocol)?;
        runtime.evaluate(FixedExport::Display)?;
        let protocol_exports = match runtime.kind {
            PackageKind::Http => HTTP_PROTOCOL_EXPORTS,
            PackageKind::Socket => SOCKET_PROTOCOL_EXPORTS,
        };
        runtime.cache_exports(protocol_exports)?;
        runtime.cache_exports(DISPLAY_EXPORTS)?;
        Ok(runtime)
    }

    /// Calls `hooks.upstream.frame` through the fixed Sidecar export mapping.
    pub fn upstream_frame(
        &mut self,
        params: FrameParams,
    ) -> Result<FrameResult, LocalSidecarError> {
        self.ensure_socket_frame()?;
        let FrameParams { buffer } = params;
        let buffer_len = buffer.bytes().len();
        let argument = self.socket_frame_params(&buffer)?;
        let result = self.call_value("upstreamFrame", argument)?;
        let frame: FrameResult = deserialize_result(result)?;
        frame
            .validate_against_buffer_len(buffer_len)
            .map_err(|error| LocalSidecarError::new(error.to_string()))?;
        Ok(frame)
    }

    /// Calls `hooks.downstream.frame` through the fixed Sidecar export mapping.
    pub fn downstream_frame(
        &mut self,
        params: FrameParams,
    ) -> Result<FrameResult, LocalSidecarError> {
        self.ensure_socket_frame()?;
        let FrameParams { buffer } = params;
        let buffer_len = buffer.bytes().len();
        let argument = self.socket_frame_params(&buffer)?;
        let result = self.call_value("downstreamFrame", argument)?;
        let frame: FrameResult = deserialize_result(result)?;
        frame
            .validate_against_buffer_len(buffer_len)
            .map_err(|error| LocalSidecarError::new(error.to_string()))?;
        Ok(frame)
    }

    /// Calls `hooks.upstream.decode` through the fixed Sidecar export mapping.
    pub fn upstream_decode(&mut self, params: DecodeParams) -> Result<Document, LocalSidecarError> {
        self.decode("upstreamDecode", params)
    }

    /// Calls `hooks.downstream.decode` through the fixed Sidecar export mapping.
    pub fn downstream_decode(
        &mut self,
        params: DecodeParams,
    ) -> Result<Document, LocalSidecarError> {
        self.decode("downstreamDecode", params)
    }

    /// Calls `hooks.upstream.encode` through the fixed Sidecar export mapping.
    pub fn upstream_encode(&mut self, params: EncodeParams) -> Result<String, LocalSidecarError> {
        self.encode("upstreamEncode", params)
    }

    /// Calls `hooks.downstream.encode` through the fixed Sidecar export mapping.
    pub fn downstream_encode(&mut self, params: EncodeParams) -> Result<String, LocalSidecarError> {
        self.encode("downstreamEncode", params)
    }

    /// Calls `document.upstream.display` through the fixed Sidecar export mapping.
    pub fn upstream_display(&mut self, params: DisplayParams) -> Result<String, LocalSidecarError> {
        self.display("upstreamDisplay", params)
    }

    /// Calls `document.downstream.display` through the fixed Sidecar export mapping.
    pub fn downstream_display(
        &mut self,
        params: DisplayParams,
    ) -> Result<String, LocalSidecarError> {
        self.display("downstreamDisplay", params)
    }

    fn decode(
        &mut self,
        export: &'static str,
        params: DecodeParams,
    ) -> Result<Document, LocalSidecarError> {
        let params = self.decode_params(params)?;
        let result = self.call_value(export, params)?;
        deserialize_result(result)
    }

    fn encode(
        &mut self,
        export: &'static str,
        params: EncodeParams,
    ) -> Result<String, LocalSidecarError> {
        let params = self.encode_params(params)?;
        match self.kind {
            PackageKind::Http => {
                let result = self.call_value(export, params)?;
                deserialize_result(result)
            }
            PackageKind::Socket => {
                let value = self.call_raw(export, params)?;
                let bytes = self.uint8_array_bytes(&value)?;
                Ok(CanonicalBase64::from_bytes(&bytes).as_str().to_owned())
            }
        }
    }

    fn display(
        &mut self,
        export: &'static str,
        params: DisplayParams,
    ) -> Result<String, LocalSidecarError> {
        let params = serde_json::to_value(params)
            .map_err(|error| LocalSidecarError::new(error.to_string()))?;
        self.call_json(export, &params).and_then(deserialize_result)
    }

    fn evaluate(&mut self, module: FixedExport) -> Result<(), LocalSidecarError> {
        let module = self.module(module);
        let promise = module.load_link_evaluate(&mut self.context);
        self.context.run_jobs()?;
        match promise.state() {
            PromiseState::Fulfilled(value) if value.is_undefined() => Ok(()),
            PromiseState::Fulfilled(_) => Err(LocalSidecarError::new(
                "package module evaluation returned an unexpected value",
            )),
            PromiseState::Rejected(reason) => Err(LocalSidecarError::new(format!(
                "package module evaluation rejected: {}",
                reason.display()
            ))),
            PromiseState::Pending => Err(LocalSidecarError::new(
                "package module evaluation did not complete",
            )),
        }
    }

    fn cache_exports(
        &mut self,
        exports: &[(&'static str, FixedExport)],
    ) -> Result<(), LocalSidecarError> {
        for (name, module) in exports {
            let value = self
                .module(*module)
                .get_value(js_string!(*name), &mut self.context)?;
            let callable = value.as_object().is_some_and(|object| object.is_callable());
            if !callable {
                return Err(LocalSidecarError::new(format!(
                    "required package export is not callable: {name}"
                )));
            }
            self.exports.insert(
                name,
                value
                    .as_object()
                    .expect("callable JavaScript values are objects")
                    .clone(),
            );
        }
        Ok(())
    }

    fn call_json(&mut self, export: &str, params: &Value) -> Result<Value, LocalSidecarError> {
        let params = self.params_value(params)?;
        self.call_value(export, params)
    }

    fn call_value(&mut self, export: &str, params: JsValue) -> Result<Value, LocalSidecarError> {
        let value = self.call_raw(export, params)?;
        value
            .to_json(&mut self.context)?
            .ok_or_else(|| LocalSidecarError::new("package export returned undefined"))
    }

    fn call_raw(&mut self, export: &str, params: JsValue) -> Result<JsValue, LocalSidecarError> {
        let function =
            self.exports.get(export).cloned().ok_or_else(|| {
                LocalSidecarError::new(format!("missing callable export: {export}"))
            })?;
        let value = function.call(&JsValue::undefined(), &[params], &mut self.context)?;
        let promise = value.as_promise();
        let Some(promise) = promise else {
            self.context.run_jobs()?;
            return Ok(value);
        };
        loop {
            self.context.run_jobs()?;
            match promise.state() {
                PromiseState::Fulfilled(value) => return Ok(value),
                PromiseState::Rejected(reason) => {
                    return Err(LocalSidecarError::new(format!(
                        "package hook promise rejected: {}",
                        reason.display()
                    )));
                }
                PromiseState::Pending => {}
            }
        }
    }

    fn params_value(&mut self, params: &Value) -> Result<JsValue, LocalSidecarError> {
        JsValue::from_json(params, &mut self.context).map_err(LocalSidecarError::from)
    }

    fn decode_params(&mut self, params: DecodeParams) -> Result<JsValue, LocalSidecarError> {
        match self.kind {
            PackageKind::Http => {
                let value = serde_json::to_value(params)
                    .map_err(|error| LocalSidecarError::new(error.to_string()))?;
                self.params_value(&value)
            }
            PackageKind::Socket => {
                let bytes = CanonicalBase64::try_from(params.input)
                    .map_err(|error| LocalSidecarError::new(error.to_string()))?
                    .bytes();
                let object = JsObject::with_object_proto(self.context.intrinsics());
                let input = self.uint8_array(&bytes)?;
                object.set(js_string!("input"), input, true, &mut self.context)?;
                Ok(object.into())
            }
        }
    }

    fn encode_params(&mut self, params: EncodeParams) -> Result<JsValue, LocalSidecarError> {
        match self.kind {
            PackageKind::Http => {
                let value = serde_json::to_value(params)
                    .map_err(|error| LocalSidecarError::new(error.to_string()))?;
                self.params_value(&value)
            }
            PackageKind::Socket => {
                let original_input = CanonicalBase64::try_from(params.original_input)
                    .map_err(|error| LocalSidecarError::new(error.to_string()))?
                    .bytes();
                let document_value = serde_json::to_value(params.document)
                    .map_err(|error| LocalSidecarError::new(error.to_string()))?;
                let document = self.params_value(&document_value)?;
                let object = JsObject::with_object_proto(self.context.intrinsics());
                let input = self.uint8_array(&original_input)?;
                object.set(js_string!("originalInput"), input, true, &mut self.context)?;
                object.set(js_string!("document"), document, true, &mut self.context)?;
                Ok(object.into())
            }
        }
    }

    fn socket_frame_params(
        &mut self,
        buffer: &CanonicalBase64,
    ) -> Result<JsValue, LocalSidecarError> {
        let object = JsObject::with_object_proto(self.context.intrinsics());
        let buffer = self.uint8_array(&buffer.bytes())?;
        object.set(js_string!("buffer"), buffer, true, &mut self.context)?;
        Ok(object.into())
    }

    fn uint8_array(&mut self, bytes: &[u8]) -> Result<JsValue, LocalSidecarError> {
        JsUint8Array::from_iter(bytes.iter().copied(), &mut self.context)
            .map(Into::into)
            .map_err(LocalSidecarError::from)
    }

    fn uint8_array_bytes(&mut self, value: &JsValue) -> Result<Vec<u8>, LocalSidecarError> {
        let object = value
            .as_object()
            .ok_or_else(|| LocalSidecarError::new("Socket encode must return Uint8Array"))?;
        JsUint8Array::from_object(object)
            .and_then(|array| array.to_vec(&mut self.context))
            .map_err(|_| LocalSidecarError::new("Socket encode must return Uint8Array"))
    }

    fn module(&self, module: FixedExport) -> Module {
        match module {
            FixedExport::Protocol => self.protocol.clone(),
            FixedExport::Display => self.display.clone(),
        }
    }

    fn ensure_socket_frame(&self) -> Result<(), LocalSidecarError> {
        if self.kind == PackageKind::Socket {
            Ok(())
        } else {
            Err(LocalSidecarError::new(
                "HTTP packages do not expose Socket frame hooks",
            ))
        }
    }
}

fn deserialize_result<T: DeserializeOwned>(value: Value) -> Result<T, LocalSidecarError> {
    serde_json::from_value(value).map_err(|error| LocalSidecarError::new(error.to_string()))
}

/// Marker callable by the generic Sidecar executable without starting lifecycle policy.
pub fn sidecar_executable_marker() {}
