//! Pure diagnostic classification and user guidance for the read-only MCP surface.

pub(super) struct DiagnosticGuidance {
    pub category: &'static str,
    pub ui_path: &'static str,
    pub action: &'static str,
    pub app_action: &'static str,
    pub alternatives: &'static [&'static str],
    pub verification: &'static str,
}

pub(super) fn diagnostic_guidance(evidence: &str) -> DiagnosticGuidance {
    if contains_any(evidence, &["tls", "certificate", "证书", "trust anchor"]) {
        return DiagnosticGuidance {
            category: "tls",
            ui_path: "入口配置 > App 接入安全 / Server 上游",
            action: "核对报错方向的证书链、主机名和信任来源；如应用使用自定义信任策略，也一并核对。",
            app_action: "检查 App 的 Network Security Config、自定义 TrustManager 和证书固定；测试构建可显式信任测试 CA，但不要在生产构建关闭证书校验。",
            alternatives: &[
                "如果不能改 App 信任策略，改用不解密 TLS 的透明转发，仅观察连接元数据。",
                "如果只需验证上游，绕过 App 端并用独立测试客户端复现同一 TLS 条件。",
            ],
            verification: "在应用中重新执行对应连接测试，并确认 TCP 与 TLS 结果分别成功。",
        };
    }
    if contains_any(evidence, &["address in use", "bind", "端口", "listen"]) {
        return DiagnosticGuidance {
            category: "bind",
            ui_path: "入口配置 > 监听地址与端口",
            action: "检查同一地址和端口是否已被其他入口或进程占用，然后修改冲突配置。",
            app_action: "确认 App 配置的代理地址和端口与当前入口一致，并检查是否同时启用了系统代理与透明路由。",
            alternatives: &["改用未占用的测试端口并同步 App 配置。"],
            verification: "重新启动该入口并确认状态为运行中。",
        };
    }
    if contains_any(evidence, &["dns", "resolve", "解析主机"]) {
        return DiagnosticGuidance {
            category: "dns",
            ui_path: "入口配置 > Server 上游",
            action: "核对 Server 主机名、当前电脑 DNS 和网络可达性。",
            app_action: "确认 App 发送的目标主机名、SNI 和 Host 一致；不要用 IP 替换域名后仍期待原证书通过主机名校验。",
            alternatives: &["在测试环境固定可解析的域名，或修正设备 DNS。"],
            verification: "重新执行 Server 连接测试并确认 DNS 与 TCP 分阶段成功。",
        };
    }
    if contains_any(evidence, &["timeout", "超时"]) {
        return DiagnosticGuidance {
            category: "timeout",
            ui_path: "设置 > 超时与容量",
            action: "先确认超时发生在连接、写入还是读取阶段，再核对对应上游可达性和超时值。",
            app_action: "检查 App 自身的连接、写入、读取超时和重试策略；先避免重复重试造成并发请求，再按失败阶段调整单一超时。",
            alternatives: &["保留原超时，只对可重复的测试请求增加一次性诊断超时。"],
            verification: "复现请求并比较新的诊断阶段与耗时。",
        };
    }
    if contains_any(evidence, &["frame", "decode", "encode", "schema", "协议包"]) {
        return DiagnosticGuidance {
            category: "protocol_package",
            ui_path: "协议包 > 版本详情；规则 > Socket",
            action: "核对入口绑定的精确包版本、方向 Schema、字段类型和报错入口。修改包时提升 SemVer 后重新导入。",
            app_action: "核对 App 的分帧方式、长度单位、字符集、字段编码和协议版本；这些必须与入口所选协议包一致。",
            alternatives: &["先用原样转发确认字节链路，再逐步启用协议解析和规则。"],
            verification: "用同一测试报文复现，并确认 Frame、解析、规则、编码和写出阶段依次成功。",
        };
    }
    DiagnosticGuidance {
        category: "general",
        ui_path: "日志 > 诊断详情",
        action: "按诊断中的对象、方向和阶段检查配置；不要把下层成功当作业务成功。",
        app_action: "在 App 中记录请求目标、时间、错误类型和关联 ID，并与代理抓包时间线对齐后再决定修改点。",
        alternatives: &["用最小测试客户端复现，以区分 App 实现问题和环境问题。"],
        verification: "再次复现并比较错误码、阶段、时间和对象是否变化。",
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::diagnostic_guidance;

    #[test]
    fn classifies_supported_failure_families_and_returns_complete_guidance() {
        for (evidence, category) in [
            ("TLS certificate rejected", "tls"),
            ("address in use while bind", "bind"),
            ("DNS resolve failed", "dns"),
            ("read timeout", "timeout"),
            ("schema decode failed", "protocol_package"),
            ("unexpected failure", "general"),
        ] {
            let guidance = diagnostic_guidance(&evidence.to_lowercase());
            assert_eq!(guidance.category, category);
            assert!(!guidance.ui_path.is_empty());
            assert!(!guidance.action.is_empty());
            assert!(!guidance.app_action.is_empty());
            assert!(!guidance.alternatives.is_empty());
            assert!(!guidance.verification.is_empty());
        }
    }
}
