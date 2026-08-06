use super::*;

impl InstallationServerIdentityProvider for CertificateServiceAdapter {
    fn load_installation_server_identity(&self) -> AppResult<ReverseClientIdentity> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&[ROOT, LEAF])?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            AppError::new("CERTIFICATE_NOT_READY", "固定测试 Root CA 尚未初始化。")
        })?;
        let leaf = snapshot
            .materials
            .get(LEAF)
            .ok_or_else(|| AppError::new("CERTIFICATE_NOT_READY", "本机叶子证书尚未签发。"))?;
        let expected_sans = leaf
            .sans
            .iter()
            .map(|san| {
                san.trim_start_matches("DNS:")
                    .trim_start_matches("IP:")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .and_then(|_| {
                self.certificates.validate_leaf(
                    &root.certificate_der,
                    &leaf.certificate_der,
                    &leaf.private_key_der,
                    &expected_sans,
                )
            })
            .map_err(app_error)?;
        Ok(ReverseClientIdentity {
            certificate_chain_der: vec![leaf.certificate_der.clone()],
            private_key_pkcs8_der: Zeroizing::new(leaf.private_key_der.clone()),
        })
    }
}

#[async_trait]
impl CertificateServicePort for CertificateServiceAdapter {
    async fn status(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        self.status_locked()
    }

    async fn synchronize_installation_ca(
        &self,
        fallback_sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        let Some(fixed_root) = self.fixed_root_bundle()? else {
            if snapshot.materials.contains_key(ROOT) && snapshot.materials.contains_key(LEAF) {
                return self.overview_locked();
            }
            self.generate_locked(&fallback_sans, snapshot)?;
            return self.overview_locked();
        };

        let root_is_current = snapshot.materials.get(ROOT).is_some_and(|stored| {
            stored.certificate_der == fixed_root.certificate_der
                && self
                    .certificates
                    .validate_root(&stored.certificate_der, &stored.private_key_der)
                    .is_ok()
        });
        if root_is_current && snapshot.materials.contains_key(LEAF) {
            return self.overview_locked();
        }

        // 升级旧安装时保留原叶子证书覆盖的 IP/DNS，只替换签发链。这样 Windows、
        // macOS 更新后无需重新填写监听 SAN，同时测试客户端只需信任一张固定 Root。
        let existing_sans = snapshot
            .materials
            .get(LEAF)
            .map(|leaf| {
                leaf.sans
                    .iter()
                    .map(|san| {
                        san.trim_start_matches("DNS:")
                            .trim_start_matches("IP:")
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|sans| !sans.is_empty())
            .unwrap_or(fallback_sans);
        self.generate_locked(&existing_sans, snapshot)?;
        self.overview_locked()
    }

    async fn overview(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        self.overview_locked()
    }

    async fn generate_ca(&self, sans: Vec<String>) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        if snapshot.materials.contains_key(ROOT) || snapshot.materials.contains_key(LEAF) {
            let labels = self.certificate_policy().labels();
            return Err(AppError::new(
                "CERTIFICATE_ALREADY_EXISTS",
                labels.already_exists_message,
            ));
        }
        self.generate_locked(&sans, snapshot)?;
        self.overview_locked()
    }

    async fn export_ca(&self) -> AppResult<OperationResultViewModel> {
        let _guard = self.material_lock.lock();
        let labels = self.certificate_policy().labels();
        let snapshot = self.load_snapshot(&[ROOT])?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_INITIALIZED",
                "固定测试 Root CA 尚未初始化，请重新打开应用或恢复测试证书。",
            )
        })?;
        let Some(selection) = self
            .dialog
            .choose_save_file("root_ca", "intercept-proxy-root-ca.crt")?
        else {
            return Ok(cancelled(labels.export_cancelled_message));
        };
        infra(self.exporter.write(
            &selection.path,
            &certificate_der_to_pem(&root.certificate_der),
            selection.overwrite_confirmed,
        ))?;
        Ok(OperationResultViewModel::success(
            labels.export_success_message,
        ))
    }

    async fn reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        verify_revision(snapshot.revision, expected_revision)?;
        let root = snapshot.materials.get(ROOT).cloned().ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_INITIALIZED",
                "固定测试 Root CA 尚未初始化。",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .map_err(app_error)?;
        let request = leaf_request(&sans)?;
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_der, &request)
            .map_err(app_error)?;
        snapshot.materials.insert(
            LEAF.into(),
            from_bundle(snapshot.revision.saturating_add(1), &leaf),
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn import_pkcs12(&self, password: String) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let Some(path) = self.dialog.choose_open_file("pkcs12")? else {
            return self.overview_locked();
        };
        let password = Zeroizing::new(password);
        let bytes = Zeroizing::new(infra(
            self.exporter.read_bounded(&path, PKCS12_IMPORT_MAX_BYTES),
        )?);
        let parsed = self
            .certificates
            .parse_pkcs12(&bytes, &password)
            .map_err(app_error)?;
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        snapshot.materials.insert(
            PKCS12.into(),
            ProtectedMaterial {
                revision: snapshot.revision.saturating_add(1),
                certificate_der: parsed.certificate_der.clone(),
                private_key_der: parsed.private_key_pkcs8_der.to_vec(),
                chain_der: parsed.chain_der.clone(),
                subject: parsed.metadata.subject.clone(),
                fingerprint: parsed.metadata.fingerprint_sha256.clone(),
                sans: parsed.metadata.san.clone(),
                not_before: parsed.metadata.not_before.clone(),
                not_after: parsed.metadata.not_after.clone(),
            },
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let Some(path) = self.dialog.choose_open_file("upstream_ca")? else {
            return self.overview_locked();
        };
        let bytes = infra(self.exporter.read_bounded(&path, CA_IMPORT_MAX_BYTES))?;
        let parsed = self
            .certificates
            .parse_upstream_ca(&bytes)
            .map_err(app_error)?;
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        snapshot.materials.insert(
            UPSTREAM_CA.into(),
            ProtectedMaterial {
                revision: snapshot.revision.saturating_add(1),
                certificate_der: parsed.certificate_der,
                private_key_der: Vec::new(),
                chain_der: Vec::new(),
                subject: parsed.metadata.subject,
                fingerprint: parsed.metadata.fingerprint_sha256,
                sans: parsed.metadata.san,
                not_before: parsed.metadata.not_before,
                not_after: parsed.metadata.not_after,
            },
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn validate(&self) -> AppResult<CertificateValidationViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        let upstream = match snapshot.materials.get(UPSTREAM_CA).cloned() {
            Some(material) => Some(material),
            None => self.bundled_upstream_material(snapshot.revision)?,
        };
        let materials = MATERIAL_KINDS
            .into_iter()
            .map(|kind| {
                let material = if kind == UPSTREAM_CA {
                    upstream.clone()
                } else {
                    snapshot.materials.get(kind).cloned()
                };
                (kind, "", "", material)
            })
            .collect::<Vec<_>>();
        let field_errors = self.configuration_errors(&materials);
        Ok(FieldValidationViewModel {
            valid: field_errors.is_empty(),
            field_errors,
            warnings: Vec::new(),
        })
    }

    async fn reset_ca(&self, expected_revision: u64) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        verify_revision(snapshot.revision, expected_revision)?;
        let sans = snapshot
            .materials
            .get(LEAF)
            .map(|leaf| {
                leaf.sans
                    .iter()
                    .map(|san| {
                        san.trim_start_matches("DNS:")
                            .trim_start_matches("IP:")
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.generate_locked(&sans, snapshot)?;
        self.overview_locked()
    }
}

/// 将存储中的 DER Root CA 转为标准 PEM。每 64 个 Base64 字符换行，便于 Android、
/// 浏览器和命令行工具直接导入；这里只编码公开证书，绝不会接触或导出私钥。
fn certificate_der_to_pem(der: &[u8]) -> Vec<u8> {
    let encoded = STANDARD.encode(der);
    let mut pem = Vec::with_capacity(encoded.len() + 64);
    pem.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.extend_from_slice(line);
        pem.push(b'\n');
    }
    pem.extend_from_slice(b"-----END CERTIFICATE-----\n");
    pem
}

#[async_trait]
impl TlsMaterialProvider for CertificateServiceAdapter {
    async fn load_epoch_snapshot(
        &self,
        leaf_sans: &[String],
    ) -> intercept_proxy_runtime::Result<TlsMaterialSnapshot> {
        let _guard = self.material_lock.lock();
        let snapshot = self
            .load_snapshot(&MATERIAL_KINDS)
            .map_err(proxy_app_error)?;
        let leaf = snapshot.materials.get(LEAF).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "Proxy leaf certificate missing",
            )
        })?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "installation Root CA missing",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .and_then(|_| {
                self.certificates.validate_leaf(
                    &root.certificate_der,
                    &leaf.certificate_der,
                    &leaf.private_key_der,
                    leaf_sans,
                )
            })
            .map_err(proxy_infra_error)?;
        let shared = snapshot.materials.get(PKCS12).ok_or_else(|| {
            ProxyError::new(ProxyErrorCode::CertificateNotReady, "shared PKCS12 missing")
        })?;
        let upstream = match snapshot.materials.get(UPSTREAM_CA).cloned() {
            Some(material) => material,
            None => self
                .bundled_upstream_material(snapshot.revision)
                .map_err(proxy_app_error)?
                .ok_or_else(|| {
                    ProxyError::new(ProxyErrorCode::CertificateNotReady, "upstream CA missing")
                })?,
        };
        let app_client_ca_der = shared
            .chain_der
            .last()
            .cloned()
            .unwrap_or_else(|| shared.certificate_der.clone());
        self.certificates
            .validate_client_identity(
                &shared.certificate_der,
                &shared.private_key_der,
                &shared.chain_der,
            )
            .map_err(proxy_infra_error)?;
        self.certificates
            .validate_client_trust_anchor_der(&app_client_ca_der)
            .map_err(proxy_infra_error)?;
        self.certificates
            .validate_ca_der(&upstream.certificate_der)
            .map_err(proxy_infra_error)?;
        Ok(TlsMaterialSnapshot {
            server_certificate_chain_der: vec![leaf.certificate_der.clone()],
            server_private_key_pkcs8_der: Zeroizing::new(leaf.private_key_der.clone()),
            app_client_ca_der,
            allowed_app_client_fingerprint: Some(parse_fingerprint(&shared.fingerprint)?),
            upstream_client_certificate_chain_der: std::iter::once(shared.certificate_der.clone())
                .chain(shared.chain_der.clone())
                .collect(),
            upstream_client_private_key_pkcs8_der: Zeroizing::new(shared.private_key_der.clone()),
            upstream_ca_der: upstream.certificate_der.clone(),
        })
    }
}

impl MitmCertificateAuthority for CertificateServiceAdapter {
    fn issue_server_identity(
        &self,
        authority_host: &str,
    ) -> intercept_proxy_runtime::Result<MitmServerIdentity> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&[ROOT]).map_err(proxy_app_error)?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "installation Root CA missing",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .map_err(proxy_infra_error)?;
        let parsed_ip = authority_host.parse::<IpAddr>().ok();
        let request = LeafCertificateRequest {
            common_name: authority_host.to_owned(),
            dns_names: if parsed_ip.is_none() {
                vec![authority_host.to_owned()]
            } else {
                Vec::new()
            },
            ip_addresses: parsed_ip.into_iter().collect(),
        };
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_der, &request)
            .map_err(proxy_infra_error)?;
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![leaf.certificate_der.clone()],
            private_key_pkcs8_der: leaf.private_key_pkcs8_der.clone(),
        })
    }
}
