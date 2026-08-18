use super::*;

/// 证书端口的测试替身单独放置，避免通用测试端口文件继续膨胀。
///
/// 这里模拟导入、查看、便携导出/恢复和清理生命周期，使配置导入测试可以覆盖
/// “新材料已提交、旧材料清理失败”等边界，而不依赖真实系统证书存储。
#[async_trait]
impl ListenerCertificateImportPort for FakePorts {
    async fn import_downstream_server_identity(
        &self,
        _label: String,
        _password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_downstream_client_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_client_identity(
        &self,
        _label: String,
        _password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_server_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn inspect(
        &self,
        reference: CertificateReference,
    ) -> AppResult<CertificateItemViewModel> {
        if self
            .discarded_certificate_references
            .lock()
            .contains(&reference.reference)
        {
            return Err(AppError::new(
                "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE",
                "托管证书材料已被清理。",
            ));
        }
        Ok(fake_certificate_overview().items.remove(0))
    }

    async fn export_portable(
        &self,
        reference: CertificateReference,
    ) -> AppResult<PortableCertificateMaterial> {
        Ok(PortableCertificateMaterial {
            reference_id: reference.id,
            label: reference.label,
            kind: reference.kind,
            material_base64: "ZmFrZS10ZXN0LWNlcnRpZmljYXRl".into(),
            material_sha256: portable_material_sha256(b"fake-test-certificate"),
            password: (reference.kind == CertificateReferenceKind::UpstreamClientIdentity)
                .then(|| "test-password".into()),
        })
    }

    async fn restore_portable(
        &self,
        material: PortableCertificateMaterial,
    ) -> AppResult<CertificateReference> {
        self.certificate_restore_calls
            .fetch_add(1, Ordering::SeqCst);
        material.validate_shape()?;
        Ok(CertificateReference {
            id: material.reference_id,
            label: material.label,
            kind: material.kind,
            reference: format!(
                "{MANAGED_LISTENER_CERTIFICATE_PREFIX}restored-{}",
                material.reference_id
            ),
        })
    }

    async fn preflight_portable(&self, material: &PortableCertificateMaterial) -> AppResult<()> {
        self.certificate_preflight_calls
            .fetch_add(1, Ordering::SeqCst);
        material.validate_shape()
    }

    async fn application_backup_baseline(&self) -> AppResult<[u8; 32]> {
        Ok([0; 32])
    }

    async fn discard(&self, reference: CertificateReference) -> AppResult<()> {
        self.certificate_discard_calls
            .fetch_add(1, Ordering::SeqCst);
        if self.block_certificate_discard.load(Ordering::SeqCst) {
            self.certificate_discard_entered.notify_one();
            self.continue_certificate_discard.notified().await;
        }
        if self.fail_certificate_discard.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "TEST_CERTIFICATE_DISCARD_FAILED",
                "测试要求证书材料清理失败。",
            ));
        }
        self.discarded_certificate_references
            .lock()
            .insert(reference.reference);
        Ok(())
    }
}
