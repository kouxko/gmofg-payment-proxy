use std::{
    env,
    error::Error,
    io::{self, Write as _},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gmofg_proxy_application::{
    AppResult, CaptureQuery, CaptureSort, ChannelKind, MessageStage, PageRequest, RuleAction,
    RuleId, RuleTerminalAction, SortDirection,
};
use gmofg_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use gmofg_proxy_infrastructure::{NativeFileDialog, adapters::FileSelection};

const TEST_RULE_PREFIX: &str = "headless-device-";

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(&self, _purpose: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Baseline,
    CustomStatus,
    Delay,
    InvalidJson,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "baseline" => Ok(Self::Baseline),
            "custom-status" => Ok(Self::CustomStatus),
            "delay" => Ok(Self::Delay),
            "invalid-json" => Ok(Self::InvalidJson),
            other => Err(format!("unsupported scenario: {other}").into()),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::CustomStatus => "custom-status",
            Self::Delay => "delay",
            Self::InvalidJson => "invalid-json",
        }
    }
}

fn capture_query(rule_id: Option<RuleId>) -> CaptureQuery {
    CaptureQuery {
        keyword: None,
        terminal_ip: Some("10.0.34.94".into()),
        channel: Some(ChannelKind::Dll),
        stage: None,
        result: None,
        rule_id,
        exceptions_only: false,
        after_event_id: None,
        sort: CaptureSort::OccurredAt,
        direction: SortDirection::Desc,
        page: PageRequest {
            page: 1,
            page_size: 200,
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let scenario = Scenario::parse(
        &env::args()
            .nth(1)
            .ok_or("usage: gmofg-headless-device-runner <scenario>")?,
    )?;
    let data_dir = env::var_os("GMOFG_APP_DATA_DIR")
        .map(PathBuf::from)
        .ok_or("GMOFG_APP_DATA_DIR is required")?;

    let host = ApplicationHostBuilder::new(
        data_dir,
        HostPlatformServices::production(Arc::new(NoFileDialog)),
    )
    .build()
    .await?;
    let application = host.application();

    let existing_rules = application.rule_list().await?;
    let foreign_enabled = existing_rules
        .iter()
        .filter(|rule| rule.enabled && !rule.name.starts_with(TEST_RULE_PREFIX))
        .map(|rule| format!("{} ({})", rule.name, rule.rule_id))
        .collect::<Vec<_>>();
    if !foreign_enabled.is_empty() {
        return Err(format!(
            "enabled non-test rules prevent headless device validation: {}",
            foreign_enabled.join(", ")
        )
        .into());
    }
    for rule in existing_rules
        .into_iter()
        .filter(|rule| rule.name.starts_with(TEST_RULE_PREFIX))
    {
        application
            .rule_delete(rule.rule_id, rule.revision, true)
            .await?;
    }

    let saved_rule = if matches!(scenario, Scenario::Baseline) {
        None
    } else {
        let mut draft = application.rule_new_draft().await?;
        draft.name = format!("{TEST_RULE_PREFIX}{}", scenario.name());
        draft.description = "Android real-device headless acceptance probe".into();
        draft.enabled = true;
        draft.channel = Some(ChannelKind::Dll);
        draft.stage = Some(MessageStage::Response);
        draft.actions = vec![match scenario {
            Scenario::CustomStatus => RuleAction::CustomHttpStatus { status: 503 },
            Scenario::Delay => RuleAction::Delay {
                milliseconds: 10_000,
            },
            Scenario::InvalidJson => RuleAction::Terminal {
                action: RuleTerminalAction::InvalidJson {
                    shift_jis_body: b"{invalid".to_vec(),
                },
            },
            Scenario::Baseline => unreachable!(),
        }];
        Some(application.rule_save(draft).await?)
    };
    let rule_id = saved_rule.as_ref().map(|rule| rule.summary.rule_id);

    let status = match application.proxy_start().await {
        Ok(status) => status,
        Err(error) => {
            if let Some(id) = rule_id
                && let Some(rule) = application
                    .rule_list()
                    .await?
                    .into_iter()
                    .find(|rule| rule.rule_id == id)
            {
                application
                    .rule_delete(rule.rule_id, rule.revision, true)
                    .await?;
            }
            host.shutdown().await?;
            return Err(error.into());
        }
    };
    println!(
        "HEADLESS_READY scenario={} epoch={} rule_id={}",
        scenario.name(),
        status
            .runtime_epoch
            .map_or_else(|| "none".into(), |epoch| epoch.to_string()),
        rule_id.map_or_else(|| "none".into(), |id| id.to_string())
    );
    io::stdout().flush()?;

    let observation = async {
        let started = Instant::now();
        let page = loop {
            let page = application
                .capture_query(capture_query(rule_id))
                .await
                .map_err(|error| error.to_string())?;
            if page
                .rows
                .iter()
                .any(|row| row.stage == MessageStage::Terminal)
            {
                break page;
            }
            if started.elapsed() >= Duration::from_secs(120) {
                return Err(format!("timed out waiting for {}", scenario.name()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let terminal = page
            .rows
            .iter()
            .find(|row| row.stage == MessageStage::Terminal)
            .ok_or_else(|| "terminal capture row missing".to_owned())?;
        let detail = application
            .capture_get_detail(terminal.session_id, terminal.runtime_epoch)
            .await
            .map_err(|error| error.to_string())?;
        let current_rule = if let Some(id) = rule_id {
            application
                .rule_list()
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|rule| rule.rule_id == id)
        } else {
            None
        };
        Ok::<_, String>(serde_json::json!({
            "scenario": scenario.name(),
            "terminal_ip": terminal.terminal_ip,
            "result": terminal.result,
            "duration_ms": terminal.duration_ms,
            "matched_rule_ids": terminal.matched_rule_ids,
            "rule_id": rule_id,
            "rule_hit_count": current_rule.as_ref().map(|rule| rule.hit_count),
            "tls_summary": detail.tls_summary,
            "rule_trace": detail.rule_trace,
        }))
    };
    tokio::pin!(observation);
    let observation_result = tokio::select! {
        result = &mut observation => result,
        signal = tokio::signal::ctrl_c() => match signal {
            Ok(()) => Err("interrupted while waiting for Android request".into()),
            Err(error) => Err(format!("failed to install interrupt handler: {error}")),
        },
    };

    let stop_error = application
        .proxy_stop()
        .await
        .err()
        .map(|error| error.to_string());
    let mut cleanup_error = None;
    if let Some(id) = rule_id {
        match application.rule_list().await {
            Ok(rules) => {
                if let Some(rule) = rules.into_iter().find(|rule| rule.rule_id == id)
                    && let Err(error) = application
                        .rule_delete(rule.rule_id, rule.revision, true)
                        .await
                {
                    cleanup_error = Some(error.to_string());
                }
            }
            Err(error) => cleanup_error = Some(error.to_string()),
        }
    }
    let remaining_rules = application.rule_list().await?;
    let remaining_test_rules = remaining_rules
        .iter()
        .filter(|rule| rule.name.starts_with(TEST_RULE_PREFIX))
        .count();
    println!(
        "HEADLESS_CLEAN scenario={} remaining_test_rules={remaining_test_rules} total_rules={}",
        scenario.name(),
        remaining_rules.len()
    );
    io::stdout().flush()?;
    host.shutdown().await?;
    if let Some(error) = stop_error {
        return Err(format!("failed to stop proxy: {error}").into());
    }
    if let Some(error) = cleanup_error {
        return Err(format!("failed to delete scenario rule: {error}").into());
    }
    let result = observation_result.map_err(|error| -> Box<dyn Error> { error.into() })?;
    println!("HEADLESS_RESULT {result}");
    io::stdout().flush()?;
    Ok(())
}
