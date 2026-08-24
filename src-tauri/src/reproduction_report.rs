//! 进程级故障复现报告组合。
//!
//! Application 层负责权威配置、运行态和结构化诊断；桌面进程额外持有持久化运行日志。
//! 这里仅把两种只读证据组合成同一份有界报告，供 MCP 查询和原生 Markdown 导出复用。

use std::fmt::Write as _;

use intercept_proxy_application::{
    AppResult, Application, DiagnosticReportQuery, DiagnosticReportViewModel,
};

use crate::runtime_logs::{ApplicationLogPage, ApplicationLogQuery, RuntimeLogStore};

const REPORT_RUNTIME_LOG_LIMIT: u16 = 200;
const REPORT_MARKDOWN_MAX_CHARS: usize = 256 * 1024;
const MARKDOWN_LOG_MESSAGE_MAX_CHARS: usize = 2_000;

#[derive(Debug)]
pub(crate) struct ReproductionReport {
    pub(crate) application: DiagnosticReportViewModel,
    pub(crate) application_logs: ApplicationLogPage,
    pub(crate) markdown: String,
}

pub(crate) async fn generate(
    application: &Application,
    runtime_logs: &RuntimeLogStore,
    query: DiagnosticReportQuery,
) -> AppResult<ReproductionReport> {
    let application = application.diagnostic_report_generate(query).await?;
    // 环境初始化、外部包注册和连接失败未必带 Listener ID。报告保留最近的全局运行日志，
    // 精确 Listener/连接过滤仍由 MCP `application_log_query` 提供稳定分页。
    let application_logs = runtime_logs.query(&ApplicationLogQuery {
        limit: REPORT_RUNTIME_LOG_LIMIT,
        ..ApplicationLogQuery::default()
    });
    let markdown = append_runtime_logs(application.markdown.clone(), &application_logs);
    Ok(ReproductionReport {
        application,
        application_logs,
        markdown,
    })
}

fn append_runtime_logs(mut markdown: String, logs: &ApplicationLogPage) -> String {
    markdown.push_str("\n## 持久化应用运行日志\n\n");
    let stored_truncated_count = logs.rows.iter().filter(|row| row.message_truncated).count();
    let report_truncated_count = logs
        .rows
        .iter()
        .filter(|row| row.message.chars().count() > MARKDOWN_LOG_MESSAGE_MAX_CHARS)
        .count();
    let retained_range = match (logs.oldest_retained_log_id, logs.newest_retained_log_id) {
        (Some(oldest), Some(newest)) => format!("{oldest}..={newest}"),
        _ => "无".into(),
    };
    let persistence_error = logs.persistence_error.as_deref().unwrap_or("无");
    let storage_path = logs.storage_path.as_deref().unwrap_or("仅内存");
    writeln!(markdown, "- has_more：{}", logs.has_more).expect("writing to String cannot fail");
    writeln!(markdown, "- 保留 ID 范围：{retained_range}").expect("writing to String cannot fail");
    writeln!(markdown, "- 容量淘汰：{} 条", logs.evicted_count)
        .expect("writing to String cannot fail");
    writeln!(markdown, "- 损坏行：{} 条", logs.corrupt_line_count)
        .expect("writing to String cannot fail");
    writeln!(markdown, "- 持久化错误：{persistence_error}").expect("writing to String cannot fail");
    writeln!(markdown, "- 存储位置：`{storage_path}`").expect("writing to String cannot fail");
    writeln!(markdown, "- 保留容量：{} 条", logs.retained_capacity)
        .expect("writing to String cannot fail");
    writeln!(markdown, "- 文件上限：{} B", logs.max_file_bytes)
        .expect("writing to String cannot fail");
    writeln!(
        markdown,
        "- 本页存储时截断消息：{stored_truncated_count} 条"
    )
    .expect("writing to String cannot fail");
    writeln!(
        markdown,
        "- 本页报告展示截断消息：{report_truncated_count} 条\n"
    )
    .expect("writing to String cannot fail");
    if logs.rows.is_empty() {
        markdown.push_str("- 当前保留范围内没有应用运行日志。\n");
    } else {
        for row in &logs.rows {
            writeln!(
                markdown,
                "- {} `[{:?}]` `{}` {}",
                row.occurred_at.to_rfc3339(),
                row.level,
                row.target,
                bounded_log_message(&row.message)
            )
            .expect("writing to String cannot fail");
        }
    }
    bounded_markdown(markdown)
}

fn bounded_log_message(message: &str) -> String {
    if message.chars().count() <= MARKDOWN_LOG_MESSAGE_MAX_CHARS {
        return message.to_owned();
    }
    message
        .chars()
        .take(MARKDOWN_LOG_MESSAGE_MAX_CHARS.saturating_sub(14))
        .chain("…[truncated]".chars())
        .collect()
}

fn bounded_markdown(markdown: String) -> String {
    if markdown.chars().count() <= REPORT_MARKDOWN_MAX_CHARS {
        return markdown;
    }
    markdown
        .chars()
        .take(REPORT_MARKDOWN_MAX_CHARS.saturating_sub(20))
        .chain("\n\n[报告已截断]\n".chars())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_logs::ApplicationLogLevel;

    #[test]
    fn runtime_logs_are_bounded_and_appended_to_the_copyable_report() {
        let store = RuntimeLogStore::memory(3);
        store.record(
            ApplicationLogLevel::Info,
            "intercept_proxy::runtime",
            "first",
        );
        store.record(
            ApplicationLogLevel::Info,
            "intercept_proxy::runtime",
            "second",
        );
        store.record(
            ApplicationLogLevel::Warning,
            "intercept_proxy::runtime",
            "third",
        );
        store.record(
            ApplicationLogLevel::Error,
            "intercept_proxy::listener",
            &"故障".repeat(40_000),
        );

        let mut page = store.query(&ApplicationLogQuery {
            limit: 2,
            ..ApplicationLogQuery::default()
        });
        page.corrupt_line_count = 2;
        page.persistence_error = Some("disk unavailable".into());

        let markdown = append_runtime_logs("# 复现报告\n".into(), &page);

        assert!(markdown.contains("## 持久化应用运行日志"));
        assert!(markdown.contains("has_more：true"));
        assert!(markdown.contains("保留 ID 范围：2..=4"));
        assert!(markdown.contains("容量淘汰：1 条"));
        assert!(markdown.contains("损坏行：2 条"));
        assert!(markdown.contains("持久化错误：disk unavailable"));
        assert!(markdown.contains("保留容量：3 条"));
        assert!(markdown.contains(&format!("文件上限：{} B", u64::MAX)));
        assert!(markdown.contains("本页存储时截断消息：1 条"));
        assert!(markdown.contains("本页报告展示截断消息：1 条"));
        assert!(markdown.contains("…[truncated]"));
        assert!(markdown.chars().count() <= REPORT_MARKDOWN_MAX_CHARS);
    }
}
