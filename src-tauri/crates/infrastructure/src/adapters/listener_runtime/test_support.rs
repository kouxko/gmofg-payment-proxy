use super::*;

#[cfg(test)]
pub(super) fn load_file_identity(
    reference: &CertificateReference,
) -> AppResult<ReverseClientIdentity> {
    let (path, password_environment) = identity_reference(&reference.reference)?;
    // 身份文件可能同时包含私钥；从文件读取开始就使用可清零缓冲，避免 PEM/P12
    // 原始材料先落入普通 Vec 再被包装。
    let bytes = read_identity_reference_file(&path)?;
    if let Some(variable) = password_environment {
        let password = Zeroizing::new(std::env::var(&variable).map_err(|_| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                format!("PKCS12 密码环境变量 {variable} 未设置。"),
            )
        })?);
        let mut parsed = CertificateService
            .parse_pkcs12(&bytes, password.as_str())
            .map_err(app_error)?;
        // ParsedPkcs12 自身实现 Drop 以清零私钥，不能直接移动字段。用 take 把所有权
        // 转交给运行时身份，并在原结构中留下空值，避免复制任何私钥缓冲。
        let mut chain = vec![std::mem::take(&mut parsed.certificate_der)];
        chain.extend(std::mem::take(&mut parsed.chain_der));
        return Ok(ReverseClientIdentity {
            certificate_chain_der: chain,
            private_key_pkcs8_der: std::mem::take(&mut parsed.private_key_pkcs8_der),
        });
    }

    let mut certificates = Cursor::new(bytes.as_slice());
    let certificate_chain_der = rustls_pemfile::certs(&mut certificates)
        .map(|entry| entry.map(|value| value.as_ref().to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AppError::new("CERTIFICATE_INVALID", format!("PEM 证书链无效：{error}"))
        })?;
    let mut private_key = Cursor::new(bytes.as_slice());
    let private_key_der = rustls_pemfile::private_key(&mut private_key)
        .map_err(|error| AppError::new("CERTIFICATE_INVALID", format!("PEM 私钥无效：{error}")))?
        .ok_or_else(|| AppError::new("CERTIFICATE_INVALID", "PEM 身份缺少私钥。"))?;
    let mut private_key_pkcs8_der =
        Zeroizing::new(Vec::with_capacity(private_key_der.secret_der().len()));
    private_key_pkcs8_der.extend_from_slice(private_key_der.secret_der());
    if certificate_chain_der.is_empty() {
        return Err(AppError::new("CERTIFICATE_INVALID", "PEM 身份缺少证书链。"));
    }
    Ok(ReverseClientIdentity {
        certificate_chain_der,
        private_key_pkcs8_der,
    })
}

#[cfg(test)]
pub(super) fn read_identity_reference_file(path: &Path) -> AppResult<Zeroizing<Vec<u8>>> {
    let mut file = fs::File::open(path).map_err(|error| {
        AppError::new(
            "CERTIFICATE_NOT_READY",
            format!("无法读取证书安全引用 {}：{error}", path.display()),
        )
    })?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.read_to_end(&mut bytes).map_err(|error| {
        AppError::new(
            "CERTIFICATE_NOT_READY",
            format!("无法读取证书安全引用 {}：{error}", path.display()),
        )
    })?;
    Ok(bytes)
}

#[cfg(test)]
pub(super) fn reference_path(reference: &str) -> AppResult<PathBuf> {
    let value = reference.strip_prefix("file:").unwrap_or(reference);
    if value.trim().is_empty() {
        return Err(AppError::new("CERTIFICATE_NOT_READY", "证书安全引用为空。"));
    }
    Ok(PathBuf::from(value))
}

#[cfg(test)]
pub(super) fn identity_reference(reference: &str) -> AppResult<(PathBuf, Option<String>)> {
    if let Some(value) = reference.strip_prefix("pkcs12:") {
        let (path, query) = value.split_once('?').ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                "PKCS12 引用必须提供 ?password_env=环境变量名。",
            )
        })?;
        let variable = query
            .strip_prefix("password_env=")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::new("CERTIFICATE_NOT_READY", "PKCS12 引用的 password_env 无效。")
            })?;
        return Ok((PathBuf::from(path), Some(variable.to_owned())));
    }
    Ok((reference_path(reference)?, None))
}
