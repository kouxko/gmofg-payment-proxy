use std::collections::BTreeMap;

use crate::{AppError, AppResult, SecretReference};

use super::Application;

impl Application {
    /// 保存 HTTP Basic 凭据并只返回安全引用。
    ///
    /// 用户名中的冒号在 Basic 语法里会改变用户名/密码边界，因此封闭失败。空密码也被
    /// 拒绝，避免用户误以为已经配置了有效的非回环代理认证。
    pub async fn workspace_secret_store_basic(
        &self,
        username: String,
        password: String,
    ) -> AppResult<SecretReference> {
        let username = username.trim().to_owned();
        if username.is_empty() {
            return Err(AppError::field(
                "CONFIG_INVALID",
                "代理认证用户名不能为空。",
                BTreeMap::from([("username".into(), vec!["请输入用户名。".into()])]),
            ));
        }
        if username.contains(':') {
            return Err(AppError::field(
                "CONFIG_INVALID",
                "代理认证用户名不能包含冒号。",
                BTreeMap::from([(
                    "username".into(),
                    vec!["HTTP Basic 用户名不能包含冒号。".into()],
                )]),
            ));
        }
        if password.is_empty() {
            return Err(AppError::field(
                "CONFIG_INVALID",
                "代理认证密码不能为空。",
                BTreeMap::from([("password".into(), vec!["请输入密码。".into()])]),
            ));
        }
        self.protected_secrets
            .store_basic_auth(username, password)
            .await
    }
}
