use intercept_proxy_domain::{DomainError, ProtocolDirection};
use wasmtime::Store;
use wasmtime::component::{
    Component, ComponentExportIndex, Instance, TypedFunc, types::ComponentItem,
};

use super::{
    HostState, PackageInstance, WasmPackageRuntime, http_bindings, invalid_component,
    runtime_guest_error, runtime_trap,
};

type HttpFinalizeResult =
    Result<String, http_bindings::intercept_proxy::protocol_package::types::PackageError>;
type HttpFinalizeFunc = TypedFunc<(String,), (HttpFinalizeResult,)>;

#[derive(Default)]
pub(super) struct HttpFinalizeExports {
    upstream: Option<HttpFinalizeFunc>,
    downstream: Option<HttpFinalizeFunc>,
}

impl HttpFinalizeExports {
    pub(super) fn has(&self, direction: ProtocolDirection) -> bool {
        self.get(direction).is_some()
    }

    pub(super) async fn call(
        &self,
        store: &mut Store<HostState>,
        direction: ProtocolDirection,
        http_json: &str,
    ) -> Result<Option<String>, DomainError> {
        let Some(function) = self.get(direction) else {
            return Ok(None);
        };
        let (result,) = function
            .call_async(store, (http_json.to_owned(),))
            .await
            .map_err(|error| runtime_trap(&error))?;
        result
            .map(Some)
            .map_err(|error| runtime_guest_error(&error.code, &error.message))
    }

    fn get(&self, direction: ProtocolDirection) -> Option<&HttpFinalizeFunc> {
        match direction {
            ProtocolDirection::Upstream => self.upstream.as_ref(),
            ProtocolDirection::Downstream => self.downstream.as_ref(),
        }
    }
}

pub(super) struct OptionalHttpFinalizeIndices {
    upstream: Option<ComponentExportIndex>,
    downstream: Option<ComponentExportIndex>,
}

impl OptionalHttpFinalizeIndices {
    pub(super) fn new(component: &Component) -> Result<Self, DomainError> {
        Ok(Self {
            upstream: optional_function_index(component, "upstream-finalize")?,
            downstream: optional_function_index(component, "downstream-finalize")?,
        })
    }

    pub(super) fn load(
        self,
        store: &mut Store<HostState>,
        instance: &Instance,
    ) -> Result<HttpFinalizeExports, DomainError> {
        Ok(HttpFinalizeExports {
            upstream: load_optional_http_finalize(
                store,
                instance,
                self.upstream,
                "upstream-finalize",
            )?,
            downstream: load_optional_http_finalize(
                store,
                instance,
                self.downstream,
                "downstream-finalize",
            )?,
        })
    }
}

fn optional_function_index(
    component: &Component,
    name: &str,
) -> Result<Option<ComponentExportIndex>, DomainError> {
    let component_type = component.component_type();
    let Some(export) = component_type.get_export(component.engine(), name) else {
        return Ok(None);
    };
    if !matches!(export.ty, ComponentItem::ComponentFunc(_)) {
        return Err(invalid_component(format!(
            "optional HTTP package export `{name}` is not a function"
        )));
    }
    Ok(component.get_export_index(None, name))
}

fn load_optional_http_finalize(
    store: &mut Store<HostState>,
    instance: &Instance,
    index: Option<ComponentExportIndex>,
    name: &str,
) -> Result<Option<HttpFinalizeFunc>, DomainError> {
    let Some(index) = index else {
        return Ok(None);
    };
    let function = instance.get_func(&mut *store, index).ok_or_else(|| {
        invalid_component(format!(
            "optional HTTP package export `{name}` is not a function"
        ))
    })?;
    function
        .typed::<(String,), (HttpFinalizeResult,)>(&mut *store)
        .map(Some)
        .map_err(|error| {
            invalid_component(format!(
                "optional HTTP package export `{name}` has an invalid signature: {error}"
            ))
        })
}

impl WasmPackageRuntime {
    /// Returns whether the HTTP package exports Finalize for one direction.
    #[must_use]
    pub fn has_http_finalize(&self, direction: ProtocolDirection) -> bool {
        match &self.instance {
            PackageInstance::Http { finalize, .. } => finalize.has(direction),
            PackageInstance::Socket { .. } => false,
        }
    }

    /// Calls an optional HTTP Finalize export for one direction.
    pub async fn finalize_http(
        &mut self,
        direction: ProtocolDirection,
        http_json: &str,
    ) -> Result<Option<String>, DomainError> {
        let PackageInstance::Http {
            store, finalize, ..
        } = &mut self.instance
        else {
            return Err(invalid_component(
                "Socket protocol packages do not export HTTP finalize hooks",
            ));
        };
        finalize.call(store, direction, http_json).await
    }
}
