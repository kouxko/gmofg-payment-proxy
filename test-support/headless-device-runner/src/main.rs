use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use gmofg_proxy_application::{
    AppResult, BreakpointDecision, BreakpointDecisionKind, BreakpointState, CaptureQuery,
    CaptureSort, ChannelKind, FaultConfigurationDraft, FaultParameterValue, MessageStage,
    PageRequest, RuleAction, RuleCondition, RuleDraft, RuleId, RuleMatchField, RuleMatchOperator,
    RuleTerminalAction, SortDirection, UiEventPayload,
};
use gmofg_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use gmofg_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use zeroize::Zeroizing;

const TEST_RULE_PREFIX: &str = "headless-device-";
const ENVELOPE_MAGIC: &[u8; 5] = b"GMPK1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const AAD: &[u8] = b"gmofg-payment-proxy/keychain-envelope/v1";

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

/// Non-interactive adapter for headless macOS validation.
///
/// The shell harness exports the existing login-Keychain master key into a
/// mode-0600 temporary file via the trusted `security` process. The runner
/// reads it once, zeroizes it on drop, and uses the exact production envelope
/// format without opening a Keychain authorization dialog from an unsigned
/// test binary.
struct HeadlessMasterKeyProtector {
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl std::fmt::Debug for HeadlessMasterKeyProtector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HeadlessMasterKeyProtector")
            .field("key", &"<redacted>")
            .finish()
    }
}

impl HeadlessMasterKeyProtector {
    fn from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        let encoded = Zeroizing::new(fs::read_to_string(path)?);
        let encoded = encoded.trim();
        if encoded.len() != KEY_BYTES * 2 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(
                "headless master-key file must contain exactly 64 hexadecimal digits".into(),
            );
        }
        let mut key = Zeroizing::new([0_u8; KEY_BYTES]);
        for (index, output) in key.iter_mut().enumerate() {
            let offset = index * 2;
            *output = u8::from_str_radix(&encoded[offset..offset + 2], 16)?;
        }
        Ok(Self { key })
    }

    fn cipher(&self, error: InfrastructureError) -> Result<LessSafeKey, InfrastructureError> {
        UnboundKey::new(&AES_256_GCM, self.key.as_ref())
            .map(LessSafeKey::new)
            .map_err(|_| error)
    }
}

impl SecretProtector for HeadlessMasterKeyProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let cipher = self.cipher(InfrastructureError::KeychainProtect)?;
        let mut nonce = [0_u8; NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        let mut protected = plaintext.to_vec();
        cipher
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(AAD),
                &mut protected,
            )
            .map_err(|_| InfrastructureError::KeychainProtect)?;

        let mut envelope = Vec::with_capacity(ENVELOPE_MAGIC.len() + NONCE_BYTES + protected.len());
        envelope.extend_from_slice(ENVELOPE_MAGIC);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&protected);
        Ok(envelope)
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let payload = ciphertext
            .strip_prefix(ENVELOPE_MAGIC)
            .ok_or(InfrastructureError::KeychainUnprotect)?;
        if payload.len() < NONCE_BYTES + TAG_BYTES {
            return Err(InfrastructureError::KeychainUnprotect);
        }
        let (nonce, encrypted) = payload.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce
            .try_into()
            .map_err(|_| InfrastructureError::KeychainUnprotect)?;
        let mut protected = Zeroizing::new(encrypted.to_vec());
        let cipher = self.cipher(InfrastructureError::KeychainUnprotect)?;
        let plaintext = cipher
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(AAD),
                &mut protected,
            )
            .map_err(|_| InfrastructureError::KeychainUnprotect)?;
        Ok(plaintext.to_vec())
    }
}

fn write_control_file(variable: &str, content: &str) -> Result<(), Box<dyn Error>> {
    if let Some(path) = env::var_os(variable).map(PathBuf::from) {
        fs::write(path, content)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Baseline,
    RequestSetJson,
    ResponseSetJson,
    RequestReplaceBody,
    ResponseReplaceBody,
    RequestSetHeader,
    ResponseSetHeader,
    RequestDelay,
    CustomStatus,
    Delay,
    MockResponse,
    InvalidJson,
    RejectTlsHandshake,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    DropResponseAfterRead,
    DropResponseAfterWrite,
    WrongContentLengthPositive,
    WrongContentLengthNegative,
    TruncateResponse,
    PauseRequest,
    PauseResponse,
    NthHit,
    OneShot,
    PriorityOrder,
    NonterminalCombination,
    DelayMockCombination,
    MatchTerminalIpEquals,
    MatchTerminalIpContains,
    MatchTerminalIpRegex,
    MatchCertificateContains,
    MatchCertificateRegex,
    MatchPathEquals,
    MatchPathContains,
    MatchPathRegex,
    MatchJsonEquals,
    MatchJsonContains,
    MatchJsonRegex,
    MatchNonmatch,
    MatchAnd,
    MatchAndNonmatch,
    InvalidStageAction,
    InvalidJsonPath,
    InvalidRegex,
    InvalidManagedHeader,
    InvalidNthHitZero,
    InvalidTimeoutZero,
    InvalidContentLengthDeltaZero,
    InvalidTerminalCombination,
    InvalidShiftJis,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "baseline" => Ok(Self::Baseline),
            "request-set-json" => Ok(Self::RequestSetJson),
            "response-set-json" => Ok(Self::ResponseSetJson),
            "request-replace-body" => Ok(Self::RequestReplaceBody),
            "response-replace-body" => Ok(Self::ResponseReplaceBody),
            "request-set-header" => Ok(Self::RequestSetHeader),
            "response-set-header" => Ok(Self::ResponseSetHeader),
            "request-delay" => Ok(Self::RequestDelay),
            "custom-status" => Ok(Self::CustomStatus),
            "delay" => Ok(Self::Delay),
            "mock-response" => Ok(Self::MockResponse),
            "invalid-json" => Ok(Self::InvalidJson),
            "reject-tls-handshake" => Ok(Self::RejectTlsHandshake),
            "disconnect-before-upstream" => Ok(Self::DisconnectBeforeUpstream),
            "upstream-connect-timeout" => Ok(Self::UpstreamConnectTimeout),
            "upstream-write-timeout" => Ok(Self::UpstreamWriteTimeout),
            "upstream-read-timeout" => Ok(Self::UpstreamReadTimeout),
            "drop-response-after-read" => Ok(Self::DropResponseAfterRead),
            "drop-response-after-write" => Ok(Self::DropResponseAfterWrite),
            "wrong-content-length-positive" => Ok(Self::WrongContentLengthPositive),
            "wrong-content-length-negative" => Ok(Self::WrongContentLengthNegative),
            "truncate-response" => Ok(Self::TruncateResponse),
            "pause-request" => Ok(Self::PauseRequest),
            "pause-response" => Ok(Self::PauseResponse),
            "nth-hit" => Ok(Self::NthHit),
            "one-shot" => Ok(Self::OneShot),
            "priority-order" => Ok(Self::PriorityOrder),
            "nonterminal-combination" => Ok(Self::NonterminalCombination),
            "delay-mock-combination" => Ok(Self::DelayMockCombination),
            "match-terminal-ip-equals" => Ok(Self::MatchTerminalIpEquals),
            "match-terminal-ip-contains" => Ok(Self::MatchTerminalIpContains),
            "match-terminal-ip-regex" => Ok(Self::MatchTerminalIpRegex),
            "match-certificate-contains" => Ok(Self::MatchCertificateContains),
            "match-certificate-regex" => Ok(Self::MatchCertificateRegex),
            "match-path-equals" => Ok(Self::MatchPathEquals),
            "match-path-contains" => Ok(Self::MatchPathContains),
            "match-path-regex" => Ok(Self::MatchPathRegex),
            "match-json-equals" => Ok(Self::MatchJsonEquals),
            "match-json-contains" => Ok(Self::MatchJsonContains),
            "match-json-regex" => Ok(Self::MatchJsonRegex),
            "match-nonmatch" => Ok(Self::MatchNonmatch),
            "match-and" => Ok(Self::MatchAnd),
            "match-and-nonmatch" => Ok(Self::MatchAndNonmatch),
            "invalid-stage-action" => Ok(Self::InvalidStageAction),
            "invalid-json-path" => Ok(Self::InvalidJsonPath),
            "invalid-regex" => Ok(Self::InvalidRegex),
            "invalid-managed-header" => Ok(Self::InvalidManagedHeader),
            "invalid-nth-hit-zero" => Ok(Self::InvalidNthHitZero),
            "invalid-timeout-zero" => Ok(Self::InvalidTimeoutZero),
            "invalid-content-length-delta-zero" => Ok(Self::InvalidContentLengthDeltaZero),
            "invalid-terminal-combination" => Ok(Self::InvalidTerminalCombination),
            "invalid-shift-jis" => Ok(Self::InvalidShiftJis),
            other => Err(format!("unsupported scenario: {other}").into()),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::RequestSetJson => "request-set-json",
            Self::ResponseSetJson => "response-set-json",
            Self::RequestReplaceBody => "request-replace-body",
            Self::ResponseReplaceBody => "response-replace-body",
            Self::RequestSetHeader => "request-set-header",
            Self::ResponseSetHeader => "response-set-header",
            Self::RequestDelay => "request-delay",
            Self::CustomStatus => "custom-status",
            Self::Delay => "delay",
            Self::MockResponse => "mock-response",
            Self::InvalidJson => "invalid-json",
            Self::RejectTlsHandshake => "reject-tls-handshake",
            Self::DisconnectBeforeUpstream => "disconnect-before-upstream",
            Self::UpstreamConnectTimeout => "upstream-connect-timeout",
            Self::UpstreamWriteTimeout => "upstream-write-timeout",
            Self::UpstreamReadTimeout => "upstream-read-timeout",
            Self::DropResponseAfterRead => "drop-response-after-read",
            Self::DropResponseAfterWrite => "drop-response-after-write",
            Self::WrongContentLengthPositive => "wrong-content-length-positive",
            Self::WrongContentLengthNegative => "wrong-content-length-negative",
            Self::TruncateResponse => "truncate-response",
            Self::PauseRequest => "pause-request",
            Self::PauseResponse => "pause-response",
            Self::NthHit => "nth-hit",
            Self::OneShot => "one-shot",
            Self::PriorityOrder => "priority-order",
            Self::NonterminalCombination => "nonterminal-combination",
            Self::DelayMockCombination => "delay-mock-combination",
            Self::MatchTerminalIpEquals => "match-terminal-ip-equals",
            Self::MatchTerminalIpContains => "match-terminal-ip-contains",
            Self::MatchTerminalIpRegex => "match-terminal-ip-regex",
            Self::MatchCertificateContains => "match-certificate-contains",
            Self::MatchCertificateRegex => "match-certificate-regex",
            Self::MatchPathEquals => "match-path-equals",
            Self::MatchPathContains => "match-path-contains",
            Self::MatchPathRegex => "match-path-regex",
            Self::MatchJsonEquals => "match-json-equals",
            Self::MatchJsonContains => "match-json-contains",
            Self::MatchJsonRegex => "match-json-regex",
            Self::MatchNonmatch => "match-nonmatch",
            Self::MatchAnd => "match-and",
            Self::MatchAndNonmatch => "match-and-nonmatch",
            Self::InvalidStageAction => "invalid-stage-action",
            Self::InvalidJsonPath => "invalid-json-path",
            Self::InvalidRegex => "invalid-regex",
            Self::InvalidManagedHeader => "invalid-managed-header",
            Self::InvalidNthHitZero => "invalid-nth-hit-zero",
            Self::InvalidTimeoutZero => "invalid-timeout-zero",
            Self::InvalidContentLengthDeltaZero => "invalid-content-length-delta-zero",
            Self::InvalidTerminalCombination => "invalid-terminal-combination",
            Self::InvalidShiftJis => "invalid-shift-jis",
        }
    }

    const fn template_id(self) -> Option<&'static str> {
        match self {
            Self::RequestSetJson => Some("modify_request_json"),
            Self::RequestDelay => Some("request_delay"),
            Self::CustomStatus => Some("custom_http_status"),
            Self::Delay => Some("response_delay"),
            Self::MockResponse => Some("mock_shift_jis_json"),
            Self::InvalidJson => Some("invalid_json"),
            Self::RejectTlsHandshake => Some("reject_tls_handshake"),
            Self::DisconnectBeforeUpstream => Some("disconnect_before_upstream"),
            Self::UpstreamConnectTimeout => Some("upstream_connect_timeout"),
            Self::UpstreamWriteTimeout => Some("upstream_write_timeout"),
            Self::UpstreamReadTimeout => Some("upstream_read_timeout"),
            Self::DropResponseAfterRead | Self::DropResponseAfterWrite => {
                Some("drop_upstream_response")
            }
            Self::WrongContentLengthPositive | Self::WrongContentLengthNegative => {
                Some("wrong_content_length")
            }
            Self::TruncateResponse => Some("truncate_response"),
            Self::Baseline
            | Self::ResponseSetJson
            | Self::RequestReplaceBody
            | Self::ResponseReplaceBody
            | Self::RequestSetHeader
            | Self::ResponseSetHeader
            | Self::PauseRequest
            | Self::PauseResponse
            | Self::NthHit
            | Self::OneShot
            | Self::PriorityOrder
            | Self::NonterminalCombination
            | Self::DelayMockCombination
            | Self::MatchTerminalIpEquals
            | Self::MatchTerminalIpContains
            | Self::MatchTerminalIpRegex
            | Self::MatchCertificateContains
            | Self::MatchCertificateRegex
            | Self::MatchPathEquals
            | Self::MatchPathContains
            | Self::MatchPathRegex
            | Self::MatchJsonEquals
            | Self::MatchJsonContains
            | Self::MatchJsonRegex
            | Self::MatchAnd
            | Self::MatchAndNonmatch
            | Self::MatchNonmatch
            | Self::InvalidStageAction
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidContentLengthDeltaZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis => None,
        }
    }

    const fn stage(self) -> MessageStage {
        match self {
            Self::RejectTlsHandshake => MessageStage::TlsHandshake,
            Self::RequestSetJson
            | Self::RequestReplaceBody
            | Self::RequestSetHeader
            | Self::RequestDelay
            | Self::MockResponse
            | Self::DisconnectBeforeUpstream
            | Self::UpstreamConnectTimeout
            | Self::UpstreamWriteTimeout
            | Self::UpstreamReadTimeout
            | Self::DropResponseAfterRead
            | Self::DropResponseAfterWrite
            | Self::PauseRequest
            | Self::DelayMockCombination
            | Self::MatchJsonEquals
            | Self::MatchJsonContains
            | Self::MatchJsonRegex
            | Self::MatchPathEquals
            | Self::MatchPathContains
            | Self::MatchPathRegex
            | Self::MatchAnd
            | Self::MatchAndNonmatch
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis => MessageStage::Request,
            Self::ResponseSetJson
            | Self::ResponseReplaceBody
            | Self::ResponseSetHeader
            | Self::CustomStatus
            | Self::Delay
            | Self::InvalidJson
            | Self::WrongContentLengthPositive
            | Self::WrongContentLengthNegative
            | Self::TruncateResponse
            | Self::PauseResponse
            | Self::NthHit
            | Self::OneShot
            | Self::PriorityOrder
            | Self::NonterminalCombination
            | Self::MatchTerminalIpEquals
            | Self::MatchTerminalIpContains
            | Self::MatchTerminalIpRegex
            | Self::MatchCertificateContains
            | Self::MatchCertificateRegex
            | Self::MatchNonmatch
            | Self::InvalidContentLengthDeltaZero => MessageStage::Response,
            Self::InvalidStageAction => MessageStage::TlsHandshake,
            Self::Baseline => unreachable!(),
        }
    }

    fn action(self) -> RuleAction {
        match self {
            Self::RequestSetJson => RuleAction::SetJsonField {
                path: "$.RequestID".into(),
                value_json: "\"RULE_MARKER\"".into(),
            },
            Self::ResponseSetJson => RuleAction::SetJsonField {
                path: "$.ErrorCode".into(),
                value_json: "\"R42\"".into(),
            },
            Self::RequestReplaceBody => RuleAction::ReplaceBodyText {
                text: r#"{"TransactionType":"Credit","RequestID":"RULE_MARKER"}"#.into(),
            },
            Self::ResponseReplaceBody => RuleAction::ReplaceBodyText {
                text: r#"{"RuleMarker":"RULE_MARKER"}"#.into(),
            },
            Self::RequestSetHeader => RuleAction::SetHeader {
                name: "x-gmofg-test".into(),
                value: "rule-hit".into(),
            },
            Self::ResponseSetHeader => RuleAction::SetHeader {
                name: "x-gmofg-test".into(),
                value: "rule-hit".into(),
            },
            Self::RequestDelay => RuleAction::Delay {
                milliseconds: 1_500,
            },
            Self::CustomStatus => RuleAction::CustomHttpStatus { status: 503 },
            Self::Delay => RuleAction::Delay {
                milliseconds: 10_000,
            },
            Self::MockResponse => RuleAction::Terminal {
                action: RuleTerminalAction::MockResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "application/json".into())],
                    shift_jis_body: br#"{"RuleMarker":"MOCK_RULE"}"#.to_vec(),
                },
            },
            Self::InvalidJson => RuleAction::Terminal {
                action: RuleTerminalAction::InvalidJson {
                    shift_jis_body: b"{invalid".to_vec(),
                },
            },
            Self::RejectTlsHandshake => RuleAction::Terminal {
                action: RuleTerminalAction::RejectTlsHandshake,
            },
            Self::DisconnectBeforeUpstream => RuleAction::Terminal {
                action: RuleTerminalAction::DisconnectBeforeUpstream,
            },
            Self::UpstreamConnectTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamConnectTimeout {
                    milliseconds: 1_500,
                },
            },
            Self::UpstreamWriteTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamWriteTimeout {
                    milliseconds: 1_500,
                },
            },
            Self::UpstreamReadTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamReadTimeout {
                    milliseconds: 1_500,
                },
            },
            Self::DropResponseAfterRead => RuleAction::Terminal {
                action: RuleTerminalAction::DropUpstreamResponse {
                    mode: gmofg_proxy_application::RuleDropResponseMode::ReadCompleteResponse,
                },
            },
            Self::DropResponseAfterWrite => RuleAction::Terminal {
                action: RuleTerminalAction::DropUpstreamResponse {
                    mode: gmofg_proxy_application::RuleDropResponseMode::CloseAfterRequestWrite,
                },
            },
            Self::WrongContentLengthPositive => RuleAction::Terminal {
                action: RuleTerminalAction::IncorrectContentLength { delta: 20 },
            },
            Self::WrongContentLengthNegative => RuleAction::Terminal {
                action: RuleTerminalAction::IncorrectContentLength { delta: -20 },
            },
            Self::TruncateResponse => RuleAction::Terminal {
                action: RuleTerminalAction::TruncateResponse { bytes: 8 },
            },
            Self::PauseRequest | Self::PauseResponse => RuleAction::Pause,
            Self::NthHit
            | Self::OneShot
            | Self::MatchTerminalIpEquals
            | Self::MatchTerminalIpContains
            | Self::MatchTerminalIpRegex
            | Self::MatchCertificateContains
            | Self::MatchCertificateRegex
            | Self::MatchNonmatch => RuleAction::CustomHttpStatus { status: 503 },
            Self::MatchPathEquals
            | Self::MatchPathContains
            | Self::MatchPathRegex
            | Self::MatchJsonEquals
            | Self::MatchJsonContains
            | Self::MatchJsonRegex
            | Self::MatchAnd
            | Self::MatchAndNonmatch => RuleAction::Terminal {
                action: RuleTerminalAction::MockResponse {
                    status: 503,
                    headers: vec![("content-type".into(), "application/json".into())],
                    shift_jis_body: b"{}".to_vec(),
                },
            },
            Self::PriorityOrder => RuleAction::SetJsonField {
                path: "$.ErrorCode".into(),
                value_json: "\"HIGH_PRIORITY\"".into(),
            },
            Self::NonterminalCombination => RuleAction::SetJsonField {
                path: "$.ErrorCode".into(),
                value_json: "\"COMBINED_RULE\"".into(),
            },
            Self::DelayMockCombination => RuleAction::Delay {
                milliseconds: 1_500,
            },
            Self::InvalidStageAction
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidContentLengthDeltaZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis
            | Self::Baseline => unreachable!(),
        }
    }

    fn fault_parameters(self) -> BTreeMap<String, FaultParameterValue> {
        match self {
            Self::RequestSetJson => BTreeMap::from([
                (
                    "path".into(),
                    FaultParameterValue::Text("$.RequestID".into()),
                ),
                (
                    "value".into(),
                    FaultParameterValue::Json("\"RULE_MARKER\"".into()),
                ),
            ]),
            Self::RequestDelay => {
                BTreeMap::from([("milliseconds".into(), FaultParameterValue::Integer(1_500))])
            }
            Self::Delay => {
                BTreeMap::from([("milliseconds".into(), FaultParameterValue::Integer(10_000))])
            }
            Self::CustomStatus => {
                BTreeMap::from([("status".into(), FaultParameterValue::Integer(503))])
            }
            Self::MockResponse => BTreeMap::from([
                ("status".into(), FaultParameterValue::Integer(200)),
                (
                    "body".into(),
                    FaultParameterValue::Json(r#"{"RuleMarker":"MOCK_RULE"}"#.into()),
                ),
            ]),
            Self::InvalidJson => {
                BTreeMap::from([("body".into(), FaultParameterValue::Text("{invalid".into()))])
            }
            Self::UpstreamConnectTimeout
            | Self::UpstreamWriteTimeout
            | Self::UpstreamReadTimeout => {
                BTreeMap::from([("milliseconds".into(), FaultParameterValue::Integer(1_500))])
            }
            Self::DropResponseAfterRead => BTreeMap::from([(
                "close_after_request_write".into(),
                FaultParameterValue::Boolean(false),
            )]),
            Self::DropResponseAfterWrite => BTreeMap::from([(
                "close_after_request_write".into(),
                FaultParameterValue::Boolean(true),
            )]),
            Self::WrongContentLengthPositive => {
                BTreeMap::from([("delta".into(), FaultParameterValue::Integer(20))])
            }
            Self::WrongContentLengthNegative => {
                BTreeMap::from([("delta".into(), FaultParameterValue::Integer(-20))])
            }
            Self::TruncateResponse => {
                BTreeMap::from([("bytes".into(), FaultParameterValue::Integer(8))])
            }
            Self::RejectTlsHandshake | Self::DisconnectBeforeUpstream => BTreeMap::new(),
            Self::Baseline
            | Self::ResponseSetJson
            | Self::RequestReplaceBody
            | Self::ResponseReplaceBody
            | Self::RequestSetHeader
            | Self::ResponseSetHeader
            | Self::PauseRequest
            | Self::PauseResponse
            | Self::NthHit
            | Self::OneShot
            | Self::PriorityOrder
            | Self::NonterminalCombination
            | Self::DelayMockCombination
            | Self::MatchTerminalIpEquals
            | Self::MatchTerminalIpContains
            | Self::MatchTerminalIpRegex
            | Self::MatchCertificateContains
            | Self::MatchCertificateRegex
            | Self::MatchPathEquals
            | Self::MatchPathContains
            | Self::MatchPathRegex
            | Self::MatchJsonEquals
            | Self::MatchJsonContains
            | Self::MatchJsonRegex
            | Self::MatchNonmatch
            | Self::MatchAnd
            | Self::MatchAndNonmatch
            | Self::InvalidStageAction
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidContentLengthDeltaZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis => unreachable!(),
        }
    }

    const fn expected_final_action_contains(self) -> Option<&'static str> {
        match self {
            Self::UpstreamConnectTimeout
            | Self::UpstreamWriteTimeout
            | Self::UpstreamReadTimeout => Some("injected timeout after 1500 ms"),
            Self::DropResponseAfterRead => {
                Some("upstream response intentionally dropped after complete read")
            }
            Self::DropResponseAfterWrite => {
                Some("upstream request intentionally closed after complete write")
            }
            _ => None,
        }
    }

    const fn expected_session_result(self) -> Option<&'static str> {
        match self {
            Self::DisconnectBeforeUpstream => Some("App 断开"),
            Self::UpstreamConnectTimeout
            | Self::UpstreamWriteTimeout
            | Self::UpstreamReadTimeout => Some("上游超时"),
            _ => None,
        }
    }

    const fn must_not_be_internal_error(self) -> bool {
        matches!(
            self,
            Self::RejectTlsHandshake
                | Self::DisconnectBeforeUpstream
                | Self::UpstreamConnectTimeout
                | Self::UpstreamWriteTimeout
                | Self::UpstreamReadTimeout
                | Self::DropResponseAfterRead
                | Self::DropResponseAfterWrite
                | Self::WrongContentLengthPositive
                | Self::WrongContentLengthNegative
                | Self::TruncateResponse
        )
    }

    const fn expected_semantic(self) -> &'static str {
        match self {
            Self::WrongContentLengthPositive => "declared content length delta +20",
            Self::WrongContentLengthNegative => "declared content length delta -20",
            Self::TruncateResponse => "send first 8 response body bytes then close",
            Self::UpstreamConnectTimeout => "upstream connect timeout after 1500 ms",
            Self::UpstreamWriteTimeout => "upstream write timeout after 1500 ms",
            Self::UpstreamReadTimeout => "upstream read timeout after 1500 ms",
            Self::DropResponseAfterRead => "drop after complete upstream response read",
            Self::DropResponseAfterWrite => "close after complete upstream request write",
            Self::PauseRequest => "request breakpoint queued and forwarded original",
            Self::PauseResponse => "response breakpoint queued and forwarded original",
            Self::NthHit => "second request only returns HTTP 503",
            Self::OneShot => "first request returns HTTP 503 and rule then disables",
            Self::PriorityOrder => "two matching rules execute in priority order",
            Self::NonterminalCombination => "delay, header, and JSON modification compose",
            Self::DelayMockCombination => "delay executes before terminal mock response",
            Self::MatchNonmatch | Self::MatchAndNonmatch => {
                "nonmatching condition records failed trace without action"
            }
            Self::InvalidStageAction
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidContentLengthDeltaZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis => "invalid rule is rejected before proxy starts",
            _ => "scenario-specific rule action",
        }
    }

    const fn request_count(self) -> usize {
        match self {
            Self::NthHit => 3,
            Self::OneShot => 2,
            _ => 1,
        }
    }

    const fn expected_hit_count(self) -> u64 {
        match self {
            Self::Baseline
            | Self::MatchNonmatch
            | Self::MatchAndNonmatch
            | Self::InvalidStageAction
            | Self::InvalidJsonPath
            | Self::InvalidRegex
            | Self::InvalidManagedHeader
            | Self::InvalidNthHitZero
            | Self::InvalidTimeoutZero
            | Self::InvalidContentLengthDeltaZero
            | Self::InvalidTerminalCombination
            | Self::InvalidShiftJis => 0,
            Self::PriorityOrder => 2,
            _ => 1,
        }
    }

    const fn is_invalid_config(self) -> bool {
        matches!(
            self,
            Self::InvalidStageAction
                | Self::InvalidJsonPath
                | Self::InvalidRegex
                | Self::InvalidManagedHeader
                | Self::InvalidNthHitZero
                | Self::InvalidTimeoutZero
                | Self::InvalidContentLengthDeltaZero
                | Self::InvalidTerminalCombination
                | Self::InvalidShiftJis
        )
    }

    const fn needs_breakpoint_resolution(self) -> bool {
        matches!(self, Self::PauseRequest | Self::PauseResponse)
    }

    const fn expected_invalid_field(self) -> Option<&'static str> {
        match self {
            Self::InvalidStageAction => Some("actions.0"),
            Self::InvalidJsonPath => Some("conditions.0.path"),
            Self::InvalidRegex => Some("conditions.0.regex"),
            Self::InvalidManagedHeader => Some("actions.0.name"),
            Self::InvalidNthHitZero => Some("conditions.0.nth_hit"),
            Self::InvalidTimeoutZero => Some("actions.0.milliseconds"),
            Self::InvalidContentLengthDeltaZero => Some("actions.0.delta"),
            Self::InvalidTerminalCombination => Some("actions"),
            Self::InvalidShiftJis => Some("actions.0.text"),
            _ => None,
        }
    }

    fn conditions(self) -> Vec<RuleCondition> {
        let field = |field, operator| RuleCondition::Field { field, operator };
        match self {
            Self::NthHit => vec![RuleCondition::NthHit { count: 2 }],
            Self::MatchTerminalIpEquals => vec![field(
                RuleMatchField::TerminalIp,
                RuleMatchOperator::Equals {
                    value: "10.0.34.94".into(),
                },
            )],
            Self::MatchTerminalIpContains => vec![field(
                RuleMatchField::TerminalIp,
                RuleMatchOperator::Contains {
                    value: "0.34".into(),
                },
            )],
            Self::MatchTerminalIpRegex => vec![field(
                RuleMatchField::TerminalIp,
                RuleMatchOperator::Regex {
                    pattern: r"^10\.0\.34\.94$".into(),
                },
            )],
            Self::MatchCertificateContains => vec![field(
                RuleMatchField::CertificateFingerprint,
                RuleMatchOperator::Contains { value: ":".into() },
            )],
            Self::MatchCertificateRegex => vec![field(
                RuleMatchField::CertificateFingerprint,
                RuleMatchOperator::Regex {
                    pattern: r"^[0-9A-Fa-f:]{32,}$".into(),
                },
            )],
            Self::MatchPathEquals => vec![field(
                RuleMatchField::PathOrRequestType,
                RuleMatchOperator::Equals { value: "/".into() },
            )],
            Self::MatchPathContains => vec![field(
                RuleMatchField::PathOrRequestType,
                RuleMatchOperator::Contains { value: "/".into() },
            )],
            Self::MatchPathRegex => vec![field(
                RuleMatchField::PathOrRequestType,
                RuleMatchOperator::Regex {
                    pattern: r"^/$".into(),
                },
            )],
            Self::MatchJsonEquals => vec![field(
                RuleMatchField::JsonPath {
                    path: "$.RequestID".into(),
                },
                RuleMatchOperator::Equals { value: "R".into() },
            )],
            Self::MatchJsonContains => vec![field(
                RuleMatchField::JsonPath {
                    path: "$.RequestID".into(),
                },
                RuleMatchOperator::Contains { value: "R".into() },
            )],
            Self::MatchJsonRegex => vec![field(
                RuleMatchField::JsonPath {
                    path: "$.RequestID".into(),
                },
                RuleMatchOperator::Regex {
                    pattern: r"^R$".into(),
                },
            )],
            Self::MatchNonmatch => vec![field(
                RuleMatchField::TerminalIp,
                RuleMatchOperator::Equals {
                    value: "192.0.2.1".into(),
                },
            )],
            Self::MatchAnd => vec![
                field(
                    RuleMatchField::TerminalIp,
                    RuleMatchOperator::Equals {
                        value: "10.0.34.94".into(),
                    },
                ),
                field(
                    RuleMatchField::PathOrRequestType,
                    RuleMatchOperator::Equals { value: "/".into() },
                ),
                field(
                    RuleMatchField::JsonPath {
                        path: "$.RequestID".into(),
                    },
                    RuleMatchOperator::Equals { value: "R".into() },
                ),
            ],
            Self::MatchAndNonmatch => vec![
                field(
                    RuleMatchField::TerminalIp,
                    RuleMatchOperator::Equals {
                        value: "10.0.34.94".into(),
                    },
                ),
                field(
                    RuleMatchField::PathOrRequestType,
                    RuleMatchOperator::Equals {
                        value: "/not-real".into(),
                    },
                ),
            ],
            _ => Vec::new(),
        }
    }

    fn actions(self) -> Vec<RuleAction> {
        match self {
            Self::NonterminalCombination => vec![
                RuleAction::Delay {
                    milliseconds: 1_500,
                },
                RuleAction::SetHeader {
                    name: "x-gmofg-combined".into(),
                    value: "yes".into(),
                },
                self.action(),
            ],
            Self::DelayMockCombination => vec![
                self.action(),
                RuleAction::Terminal {
                    action: RuleTerminalAction::MockResponse {
                        status: 200,
                        headers: vec![("content-type".into(), "application/json".into())],
                        shift_jis_body: br#"{"RuleMarker":"DELAYED_MOCK"}"#.to_vec(),
                    },
                },
            ],
            _ => vec![self.action()],
        }
    }

    fn invalid_draft(self, mut draft: RuleDraft) -> RuleDraft {
        draft.name = format!("{TEST_RULE_PREFIX}{}", self.name());
        draft.description = "Expected rejection probe".into();
        draft.enabled = true;
        draft.channel = Some(ChannelKind::Dll);
        draft.stage = Some(self.stage());
        match self {
            Self::InvalidStageAction => {
                draft.actions = vec![RuleAction::SetHeader {
                    name: "x-invalid".into(),
                    value: "x".into(),
                }];
            }
            Self::InvalidJsonPath => {
                draft.conditions = vec![RuleCondition::Field {
                    field: RuleMatchField::JsonPath {
                        path: "$.items[]".into(),
                    },
                    operator: RuleMatchOperator::Equals { value: "x".into() },
                }];
                draft.actions = vec![RuleAction::Delay { milliseconds: 1 }];
            }
            Self::InvalidRegex => {
                draft.conditions = vec![RuleCondition::Field {
                    field: RuleMatchField::TerminalIp,
                    operator: RuleMatchOperator::Regex {
                        pattern: "(".into(),
                    },
                }];
                draft.actions = vec![RuleAction::Delay { milliseconds: 1 }];
            }
            Self::InvalidManagedHeader => {
                draft.actions = vec![RuleAction::SetHeader {
                    name: "content-length".into(),
                    value: "1".into(),
                }];
            }
            Self::InvalidNthHitZero => {
                draft.conditions = vec![RuleCondition::NthHit { count: 0 }];
                draft.actions = vec![RuleAction::Delay { milliseconds: 1 }];
            }
            Self::InvalidTimeoutZero => {
                draft.actions = vec![RuleAction::Terminal {
                    action: RuleTerminalAction::UpstreamConnectTimeout { milliseconds: 0 },
                }];
            }
            Self::InvalidContentLengthDeltaZero => {
                draft.actions = vec![RuleAction::Terminal {
                    action: RuleTerminalAction::IncorrectContentLength { delta: 0 },
                }];
            }
            Self::InvalidTerminalCombination => {
                draft.actions = vec![
                    RuleAction::Terminal {
                        action: RuleTerminalAction::MockResponse {
                            status: 200,
                            headers: Vec::new(),
                            shift_jis_body: b"{}".to_vec(),
                        },
                    },
                    RuleAction::Delay { milliseconds: 1 },
                ];
            }
            Self::InvalidShiftJis => {
                draft.actions = vec![RuleAction::ReplaceBodyText {
                    text: "emoji \u{1F680}".into(),
                }];
            }
            _ => unreachable!(),
        }
        draft
    }
}

#[derive(Clone, Copy)]
struct CreatedRule {
    rule_id: RuleId,
    via_fault_template: bool,
}

fn action_effect_confirmed(
    scenario: Scenario,
    detail: &gmofg_proxy_application::CaptureDetailViewModel,
    duration_ms: Option<u64>,
) -> bool {
    let request_header = || {
        detail
            .request
            .headers
            .get("x-gmofg-test")
            .is_some_and(|values| values.iter().any(|value| value == "rule-hit"))
    };
    let response_header = || {
        detail
            .response
            .as_ref()
            .and_then(|response| response.headers.get("x-gmofg-test"))
            .is_some_and(|values| values.iter().any(|value| value == "rule-hit"))
    };
    let request_body_contains = |marker: &str| {
        detail
            .request
            .body_text
            .as_deref()
            .is_some_and(|body| body.contains(marker))
    };
    let response_body_contains = |marker: &str| {
        detail
            .response
            .as_ref()
            .and_then(|response| response.body_text.as_deref())
            .is_some_and(|body| body.contains(marker))
    };
    match scenario {
        Scenario::Baseline | Scenario::CustomStatus => true,
        Scenario::RequestSetJson => request_body_contains("RULE_MARKER"),
        Scenario::ResponseSetJson => response_body_contains("R42"),
        Scenario::RequestReplaceBody => request_body_contains("RULE_MARKER"),
        Scenario::ResponseReplaceBody => response_body_contains("RULE_MARKER"),
        Scenario::RequestSetHeader => request_header(),
        Scenario::ResponseSetHeader => response_header(),
        Scenario::RequestDelay => duration_ms.is_some_and(|duration| duration >= 1_500),
        Scenario::Delay => duration_ms.is_some_and(|duration| duration >= 10_000),
        // Synthetic terminal responses are not stored as upstream response
        // snapshots. Rust proves their rule hit/trace; Android verifies the
        // exact downstream body or parse failure.
        Scenario::MockResponse
        | Scenario::InvalidJson
        | Scenario::RejectTlsHandshake
        | Scenario::DisconnectBeforeUpstream
        | Scenario::UpstreamConnectTimeout
        | Scenario::UpstreamWriteTimeout
        | Scenario::UpstreamReadTimeout
        | Scenario::DropResponseAfterRead
        | Scenario::DropResponseAfterWrite
        | Scenario::WrongContentLengthPositive
        | Scenario::WrongContentLengthNegative
        | Scenario::TruncateResponse
        | Scenario::PauseRequest
        | Scenario::PauseResponse
        | Scenario::NthHit
        | Scenario::OneShot
        | Scenario::MatchTerminalIpEquals
        | Scenario::MatchTerminalIpContains
        | Scenario::MatchTerminalIpRegex
        | Scenario::MatchCertificateContains
        | Scenario::MatchCertificateRegex
        | Scenario::MatchPathEquals
        | Scenario::MatchPathContains
        | Scenario::MatchPathRegex
        | Scenario::MatchJsonEquals
        | Scenario::MatchJsonContains
        | Scenario::MatchJsonRegex
        | Scenario::MatchNonmatch
        | Scenario::MatchAnd
        | Scenario::MatchAndNonmatch
        | Scenario::InvalidStageAction
        | Scenario::InvalidJsonPath
        | Scenario::InvalidRegex
        | Scenario::InvalidManagedHeader
        | Scenario::InvalidNthHitZero
        | Scenario::InvalidTimeoutZero
        | Scenario::InvalidContentLengthDeltaZero
        | Scenario::InvalidTerminalCombination
        | Scenario::InvalidShiftJis => true,
        Scenario::PriorityOrder => {
            response_body_contains("HIGH_PRIORITY")
                && detail
                    .response
                    .as_ref()
                    .and_then(|response| response.headers.get("x-gmofg-priority-second"))
                    .is_some_and(|values| values.iter().any(|value| value == "yes"))
        }
        Scenario::NonterminalCombination => {
            response_body_contains("COMBINED_RULE")
                && detail
                    .response
                    .as_ref()
                    .and_then(|response| response.headers.get("x-gmofg-combined"))
                    .is_some_and(|values| values.iter().any(|value| value == "yes"))
                && duration_ms.is_some_and(|duration| duration >= 1_500)
        }
        Scenario::DelayMockCombination => duration_ms.is_some_and(|duration| duration >= 1_500),
    }
}

fn capture_query(rule_id: Option<RuleId>, after_event_id: Option<u64>) -> CaptureQuery {
    CaptureQuery {
        keyword: None,
        terminal_ip: Some("10.0.34.94".into()),
        channel: Some(ChannelKind::Dll),
        stage: None,
        result: None,
        rule_id,
        exceptions_only: false,
        after_event_id,
        sort: CaptureSort::OccurredAt,
        direction: SortDirection::Desc,
        page: PageRequest {
            page: 1,
            page_size: 200,
        },
    }
}

#[cfg(unix)]
async fn shutdown_signal() -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> io::Result<()> {
    tokio::signal::ctrl_c().await
}

fn finish_run(
    primary_error: Option<String>,
    cleanup_errors: Vec<String>,
    shutdown_error: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let mut failures = Vec::new();
    if let Some(error) = primary_error {
        failures.push(format!("scenario failed: {error}"));
    }
    if !cleanup_errors.is_empty() {
        failures.push(format!(
            "emergency rule cleanup failed: {}",
            cleanup_errors.join("; ")
        ));
    }
    if let Some(error) = shutdown_error {
        failures.push(format!("host shutdown failed: {error}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join(" | ").into())
    }
}

async fn wait_for_android_completion_signal() -> Result<(), String> {
    let path = env::var_os("GMOFG_ANDROID_COMPLETION_SIGNAL")
        .map(PathBuf::from)
        .ok_or_else(|| "GMOFG_ANDROID_COMPLETION_SIGNAL is required".to_owned())?;
    let started = Instant::now();
    loop {
        if path.is_file() {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(360) {
            return Err("timed out waiting for Android completion signal".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
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
    let master_key_file = env::var_os("GMOFG_HEADLESS_MASTER_KEY_FILE")
        .map(PathBuf::from)
        .ok_or("GMOFG_HEADLESS_MASTER_KEY_FILE is required")?;
    let secret_protector = Arc::new(HeadlessMasterKeyProtector::from_file(&master_key_file)?);
    write_control_file("GMOFG_HEADLESS_PHASE_FILE", "building-host\n")?;

    let host = ApplicationHostBuilder::new(
        data_dir,
        HostPlatformServices::new(secret_protector, Arc::new(NoFileDialog)),
    )
    .build()
    .await?;
    let application = host.application();
    let mut acquired_rule_ids = Vec::new();

    let run_result = async {
        let available_template_ids = application
        .fault_template_list()
        .await?
        .into_iter()
        .map(|template| template.template_id)
        .collect::<BTreeSet<_>>();
    let matrix: serde_json::Value = serde_json::from_str(include_str!("../scenarios.json"))?;
    let matrix_template_ids = matrix["scenarios"]
        .as_array()
        .ok_or("scenario matrix has no scenarios array")?
        .iter()
        .filter_map(|entry| entry["template_id"].as_str())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    if available_template_ids != matrix_template_ids {
        return Err(format!(
            "fault template matrix is not closed: available={available_template_ids:?}, \
             represented={matrix_template_ids:?}"
        )
        .into());
    }

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
    let mut subscription = application.app_subscribe_events(0)?;
    let subscription_id = subscription.subscription_id;
    let capture_cursor = application
        .capture_query(capture_query(None, None))
        .await?
        .rows
        .iter()
        .map(|row| row.event_id)
        .max();

    let mut invalid_rejection = None;
    let mut created_rules = if matches!(scenario, Scenario::Baseline) {
        Vec::new()
    } else if scenario.is_invalid_config() {
        let draft = scenario.invalid_draft(application.rule_new_draft().await?);
        let error = application
            .rule_save(draft)
            .await
            .expect_err("invalid configuration unexpectedly saved");
        if error.view_model.code != "RULE_INVALID" {
            return Err(format!(
                "{} returned error code {}, expected RULE_INVALID",
                scenario.name(),
                error.view_model.code
            )
            .into());
        }
        let expected_field = scenario
            .expected_invalid_field()
            .ok_or("invalid scenario has no expected field signature")?;
        let actual_fields = error
            .view_model
            .field_errors
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if actual_fields != [expected_field] {
            return Err(format!(
                "{} field-error signature was {:?}, expected exactly {:?}",
                scenario.name(),
                actual_fields,
                [expected_field]
            )
            .into());
        }
        invalid_rejection = Some(serde_json::json!({
            "code": error.view_model.code,
            "field_errors": error.view_model.field_errors,
            "expected_fields": [expected_field],
            "actual_fields": actual_fields,
            "field_signature_exact": true,
        }));
        Vec::new()
    } else if let Some(template_id) = scenario.template_id() {
        let active = application
            .fault_configure(FaultConfigurationDraft {
                template_id: template_id.into(),
                existing_rule_id: None,
                expected_revision: None,
                channel: Some(ChannelKind::Dll),
                terminal: if matches!(scenario, Scenario::RejectTlsHandshake) {
                    None
                } else {
                    Some("10.0.34.94".into())
                },
                target: None,
                nth_hit: Some(1),
                one_shot: false,
                priority: 100,
                parameters: scenario.fault_parameters(),
            })
            .await?;
        acquired_rule_ids.push(active.rule_id);
        let mut draft = application.rule_get(active.rule_id).await?.draft;
        draft.name = format!("{TEST_RULE_PREFIX}{}", scenario.name());
        let saved = application.rule_save(draft).await?;
        vec![CreatedRule {
            rule_id: saved.summary.rule_id,
            via_fault_template: true,
        }]
    } else {
        let mut draft = application.rule_new_draft().await?;
        draft.name = format!("{TEST_RULE_PREFIX}{}", scenario.name());
        draft.description = "Android real-device headless acceptance probe".into();
        draft.enabled = true;
        draft.priority = if matches!(scenario, Scenario::PriorityOrder) {
            10
        } else {
            100
        };
        draft.channel = Some(ChannelKind::Dll);
        draft.stage = Some(scenario.stage());
        draft.conditions = scenario.conditions();
        draft.actions = scenario.actions();
        draft.one_shot = matches!(scenario, Scenario::OneShot);
        let saved = application.rule_save(draft).await?;
        acquired_rule_ids.push(saved.summary.rule_id);
        vec![CreatedRule {
            rule_id: saved.summary.rule_id,
            via_fault_template: false,
        }]
    };
    if matches!(scenario, Scenario::PriorityOrder) {
        let mut draft = application.rule_new_draft().await?;
        draft.name = format!("{TEST_RULE_PREFIX}{}-second", scenario.name());
        draft.description = "Lower-priority companion rule".into();
        draft.enabled = true;
        draft.priority = 20;
        draft.channel = Some(ChannelKind::Dll);
        draft.stage = Some(MessageStage::Response);
        draft.actions = vec![RuleAction::SetHeader {
            name: "x-gmofg-priority-second".into(),
            value: "yes".into(),
        }];
        let saved = application.rule_save(draft).await?;
        acquired_rule_ids.push(saved.summary.rule_id);
        created_rules.push(CreatedRule {
            rule_id: saved.summary.rule_id,
            via_fault_template: false,
        });
    }
    let rule_ids = created_rules
        .iter()
        .map(|rule| rule.rule_id)
        .collect::<Vec<_>>();
    let rule_id = rule_ids.first().copied();

    write_control_file("GMOFG_HEADLESS_PHASE_FILE", "starting-proxy\n")?;
    let status = match application.proxy_start().await {
        Ok(status) => status,
        Err(error) => {
            for id in &rule_ids {
                if let Some(rule) = application
                    .rule_list()
                    .await?
                    .into_iter()
                    .find(|rule| rule.rule_id == *id)
                {
                    application
                        .rule_delete(rule.rule_id, rule.revision, true)
                        .await?;
                }
            }
            return Err(error.into());
        }
    };
    write_control_file("GMOFG_HEADLESS_PHASE_FILE", "ready\n")?;
    write_control_file(
        "GMOFG_HEADLESS_READY_FILE",
        &format!("scenario={}\n", scenario.name()),
    )?;
    println!(
        "HEADLESS_READY scenario={} epoch={} rule_id={} template_id={}",
        scenario.name(),
        status
            .runtime_epoch
            .map_or_else(|| "none".into(), |epoch| epoch.to_string()),
        if rule_ids.is_empty() {
            "none".into()
        } else {
            rule_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        scenario.template_id().unwrap_or("none")
    );
    io::stdout().flush()?;

    let observation = async {
        if matches!(scenario, Scenario::RejectTlsHandshake) {
            let started = Instant::now();
            loop {
                let remaining = Duration::from_secs(30)
                    .checked_sub(started.elapsed())
                    .ok_or_else(|| "timed out waiting for TLS RuleHit event".to_owned())?;
                let event = tokio::time::timeout(remaining, subscription.live.recv())
                    .await
                    .map_err(|_| "timed out waiting for TLS RuleHit event".to_owned())?
                    .ok_or_else(|| "event subscription closed before TLS RuleHit".to_owned())?;
                if let UiEventPayload::RuleHit(rule) = event.payload
                    && Some(rule.rule_id) == rule_id
                {
                    if rule.hit_count != 1 {
                        return Err(format!(
                            "TLS RuleHit count was {}, expected 1",
                            rule.hit_count
                        ));
                    }
                    wait_for_android_completion_signal().await?;
                    return Ok(serde_json::json!({
                        "scenario": scenario.name(),
                        "template_id": scenario.template_id(),
                        "terminal_ip": "10.0.34.94",
                        "result": "TLS handshake rejected",
                        "duration_ms": null,
                        "matched_rule_ids": [rule.rule_id],
                        "rule_id": rule.rule_id,
                        "rule_hit_count": rule.hit_count,
                        "action_effect_confirmed": true,
                        "tls_rule_hit_confirmed": true,
                        "tls_summary": "handshake rejected before HTTP",
                        "rule_trace": [format!(
                            "{} [RuleHit event] TLS handshake rule matched",
                            rule.rule_id
                        )],
                        "expected_semantic": scenario.expected_semantic(),
                        "final_action": "TLS handshake rejected by rule",
                        "template_inventory": {
                            "available": &available_template_ids,
                            "represented": &matrix_template_ids,
                            "closed": true,
                        },
                    }));
                }
            }
        }
        let started = Instant::now();
        let runtime_epoch = status
            .runtime_epoch
            .ok_or_else(|| "running proxy has no runtime epoch".to_owned())?;
        let mut resolved_breakpoints = Vec::new();
        let query_rule_id = (scenario.expected_hit_count() > 0
            && !matches!(scenario, Scenario::NthHit | Scenario::OneShot))
        .then_some(rule_id)
        .flatten();
        let page = loop {
            if scenario.needs_breakpoint_resolution() {
                for pending in application.breakpoint_query(Some(runtime_epoch)) {
                    if !resolved_breakpoints.contains(&pending.breakpoint_id) {
                        let resolved = application
                            .breakpoint_resolve(
                                runtime_epoch,
                                BreakpointDecision {
                                    breakpoint_id: pending.breakpoint_id,
                                    expected_revision: pending.revision,
                                    kind: BreakpointDecisionKind::ForwardOriginal,
                                    message: None,
                                    delay_ms: None,
                                    http_status: None,
                                    content_length_delta: None,
                                    truncate_at: None,
                                },
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                        if resolved.state != BreakpointState::Resolved {
                            return Err(format!(
                                "breakpoint {} resolved to {:?}",
                                resolved.breakpoint_id, resolved.state
                            ));
                        }
                        resolved_breakpoints.push(pending.breakpoint_id);
                    }
                }
            }
            let page = application
                .capture_query(capture_query(query_rule_id, capture_cursor))
                .await
                .map_err(|error| error.to_string())?;
            let terminal_count = page
                .rows
                .iter()
                .filter(|row| row.stage == MessageStage::Terminal)
                .count();
            if terminal_count >= scenario.request_count() {
                break page;
            }
            let timeout = Duration::from_secs(
                120_u64.saturating_mul(u64::try_from(scenario.request_count()).unwrap_or(u64::MAX)),
            );
            if started.elapsed() >= timeout {
                return Err(format!("timed out waiting for {}", scenario.name()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let terminals = page
            .rows
            .iter()
            .filter(|row| row.stage == MessageStage::Terminal)
            .take(scenario.request_count())
            .collect::<Vec<_>>();
        if terminals.len() != scenario.request_count() {
            return Err("terminal capture row count mismatch".into());
        }
        let mut details = Vec::new();
        let mut sessions = Vec::new();
        for terminal in &terminals {
            details.push(
                application
                    .capture_get_detail(terminal.session_id, terminal.runtime_epoch)
                    .await
                    .map_err(|error| error.to_string())?,
            );
            sessions.push(
                application
                    .session_get(terminal.session_id)
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
        if let Some(expected) = scenario.expected_final_action_contains()
            && !sessions
                .iter()
                .any(|session| session.final_action.contains(expected))
        {
            return Err(format!(
                "no final action for {} contained {:?}: {:?}",
                scenario.name(),
                expected,
                sessions
                    .iter()
                    .map(|session| session.final_action.as_str())
                    .collect::<Vec<_>>()
            ));
        }
        if let Some(expected) = scenario.expected_session_result()
            && !terminals.iter().any(|terminal| terminal.result == expected)
        {
            return Err(format!(
                "{} session results were {:?}, expected {:?}",
                scenario.name(),
                terminals
                    .iter()
                    .map(|terminal| terminal.result.as_str())
                    .collect::<Vec<_>>(),
                expected
            ));
        }
        if scenario.must_not_be_internal_error()
            && terminals
                .iter()
                .any(|terminal| terminal.result == "内部错误")
        {
            return Err(format!("{} was classified as 内部错误", scenario.name()));
        }
        let rules = application
            .rule_list()
            .await
            .map_err(|error| error.to_string())?;
        let hit_counts = rule_ids
            .iter()
            .map(|id| {
                rules
                    .iter()
                    .find(|rule| rule.rule_id == *id)
                    .map_or(0, |rule| rule.hit_count)
            })
            .collect::<Vec<_>>();
        let total_hit_count = hit_counts.iter().sum::<u64>();
        if total_hit_count != scenario.expected_hit_count() {
            return Err(format!(
                "{} total hit count was {}, expected {}",
                scenario.name(),
                total_hit_count,
                scenario.expected_hit_count()
            ));
        }
        if matches!(scenario, Scenario::OneShot)
            && rules
                .iter()
                .find(|rule| Some(rule.rule_id) == rule_id)
                .is_some_and(|rule| rule.enabled)
        {
            return Err("one-shot rule remained enabled after its first hit".into());
        }
        let representative_index = details
            .iter()
            .position(|detail| {
                detail
                    .rule_trace
                    .iter()
                    .any(|trace| trace.contains("[命中]"))
            })
            .unwrap_or(0);
        let detail = &details[representative_index];
        let terminal = terminals[representative_index];
        let action_effect_confirmed =
            action_effect_confirmed(scenario, detail, terminal.duration_ms);
        if !action_effect_confirmed {
            return Err(format!(
                "capture detail did not confirm action effect for {}",
                scenario.name()
            ));
        }
        let combined_trace = details
            .iter()
            .flat_map(|detail| detail.rule_trace.clone())
            .collect::<Vec<_>>();
        if matches!(
            scenario,
            Scenario::MatchNonmatch | Scenario::MatchAndNonmatch
        ) && !combined_trace
            .iter()
            .any(|trace| trace.contains("[未命中]"))
        {
            return Err(format!("{} has no failed rule trace", scenario.name()));
        }
        if matches!(scenario, Scenario::NthHit) {
            let matched = combined_trace
                .iter()
                .filter(|trace| trace.contains("[命中]"))
                .count();
            let unmatched = combined_trace
                .iter()
                .filter(|trace| trace.contains("[未命中]"))
                .count();
            if matched != 1 || unmatched != 2 {
                return Err(format!(
                    "NthHit trace sequence was matched={matched}, unmatched={unmatched}; expected 1/2"
                ));
            }
        }
        let priority_order_confirmed = if matches!(scenario, Scenario::PriorityOrder) {
            let first = rule_ids
                .first()
                .ok_or_else(|| "priority rule is missing".to_owned())?
                .to_string();
            let second = rule_ids
                .get(1)
                .ok_or_else(|| "priority companion rule is missing".to_owned())?
                .to_string();
            let first_index = combined_trace
                .iter()
                .position(|trace| trace.starts_with(&first))
                .ok_or_else(|| "priority rule trace is missing".to_owned())?;
            let second_index = combined_trace
                .iter()
                .position(|trace| trace.starts_with(&second))
                .ok_or_else(|| "priority companion trace is missing".to_owned())?;
            if first_index >= second_index {
                return Err("rule traces are not in ascending priority order".into());
            }
            true
        } else {
            false
        };
        if scenario.needs_breakpoint_resolution() && resolved_breakpoints.len() != 1 {
            return Err(format!(
                "{} resolved {} breakpoints, expected 1",
                scenario.name(),
                resolved_breakpoints.len()
            ));
        }
        Ok::<_, String>(serde_json::json!({
            "scenario": scenario.name(),
            "template_id": scenario.template_id(),
            "terminal_ip": terminal.terminal_ip,
            "result": terminal.result,
            "duration_ms": terminal.duration_ms,
            "matched_rule_ids": terminal.matched_rule_ids,
            "rule_id": rule_id,
            "rule_ids": rule_ids,
            "rule_hit_count": if rule_ids.is_empty() { None } else { Some(total_hit_count) },
            "rule_hit_counts": hit_counts,
            "action_effect_confirmed": action_effect_confirmed,
            "expected_semantic": scenario.expected_semantic(),
            "final_action": sessions
                .iter()
                .map(|session| session.final_action.clone())
                .collect::<Vec<_>>()
                .join(" | "),
            "tls_summary": detail.tls_summary,
            "rule_trace": combined_trace,
            "request_count": scenario.request_count(),
            "resolved_breakpoints": resolved_breakpoints,
            "priority_order_confirmed": priority_order_confirmed,
            "one_shot_disabled": matches!(scenario, Scenario::OneShot),
            "invalid_rejection": invalid_rejection,
            "template_inventory": {
                "available": &available_template_ids,
                "represented": &matrix_template_ids,
                "closed": true,
            },
        }))
    };
    tokio::pin!(observation);
    let observation_result = tokio::select! {
        result = &mut observation => result,
        signal = shutdown_signal() => match signal {
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
    for created_rule in &created_rules {
        let id = created_rule.rule_id;
        match application.rule_list().await {
            Ok(rules) => {
                if let Some(rule) = rules.into_iter().find(|rule| rule.rule_id == id) {
                    if created_rule.via_fault_template
                        && rule.enabled
                        && let Err(error) = application
                            .fault_stop(rule.rule_id, rule.revision, true)
                            .await
                    {
                        cleanup_error = Some(error.to_string());
                    }
                    if cleanup_error.is_none() {
                        match application.rule_list().await {
                            Ok(rules) => {
                                if let Some(current) =
                                    rules.into_iter().find(|rule| rule.rule_id == id)
                                    && let Err(error) = application
                                        .rule_delete(current.rule_id, current.revision, true)
                                        .await
                                {
                                    cleanup_error = Some(error.to_string());
                                }
                            }
                            Err(error) => cleanup_error = Some(error.to_string()),
                        }
                    }
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
    let created_rule_remaining = rule_ids
        .iter()
        .any(|id| remaining_rules.iter().any(|rule| rule.rule_id == *id));
    println!(
        "HEADLESS_CLEAN scenario={} remaining_test_rules={remaining_test_rules} \
         created_rule_remaining={} total_rules={}",
        scenario.name(),
        usize::from(created_rule_remaining),
        remaining_rules.len()
    );
    io::stdout().flush()?;
    let _ = application.app_unsubscribe_events(subscription_id);
    if let Some(error) = stop_error {
        return Err(format!("failed to stop proxy: {error}").into());
    }
    if let Some(error) = cleanup_error {
        return Err(format!("failed to delete scenario rule: {error}").into());
    }
    let result = observation_result.map_err(|error| -> Box<dyn Error> { error.into() })?;
    println!("HEADLESS_RESULT {result}");
    io::stdout().flush()?;
    Ok::<(), Box<dyn Error>>(())
    }
    .await;

    let mut emergency_cleanup_errors = Vec::new();
    match application.rule_list().await {
        Ok(rules) => {
            for rule in rules.into_iter().filter(|rule| {
                rule.name.starts_with(TEST_RULE_PREFIX) || acquired_rule_ids.contains(&rule.rule_id)
            }) {
                if let Err(error) = application
                    .rule_delete(rule.rule_id, rule.revision, true)
                    .await
                {
                    emergency_cleanup_errors.push(error.to_string());
                }
            }
        }
        Err(error) => emergency_cleanup_errors.push(error.to_string()),
    }
    let shutdown_result = host.shutdown().await;

    finish_run(
        run_result.err().map(|error| error.to_string()),
        emergency_cleanup_errors,
        shutdown_result.err().map(|error| error.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::{Scenario, finish_run};

    #[test]
    fn every_ready_matrix_scenario_is_supported_by_the_runner() {
        let matrix: serde_json::Value =
            serde_json::from_str(include_str!("../scenarios.json")).expect("matrix JSON");
        let scenarios = matrix["scenarios"].as_array().expect("scenario array");
        assert!(scenarios.iter().all(|scenario| {
            scenario["implementation"] == "ready"
                && Scenario::parse(scenario["id"].as_str().expect("scenario id")).is_ok()
        }));
    }

    #[test]
    fn stateful_scenarios_observe_the_complete_request_sequence() {
        assert_eq!(Scenario::NthHit.request_count(), 3);
        assert_eq!(Scenario::OneShot.request_count(), 2);
        assert!(Scenario::PauseRequest.needs_breakpoint_resolution());
        assert!(Scenario::PauseResponse.needs_breakpoint_resolution());
    }

    #[test]
    fn invalid_configuration_scenarios_never_create_runtime_rules() {
        for scenario in [
            Scenario::InvalidStageAction,
            Scenario::InvalidJsonPath,
            Scenario::InvalidRegex,
            Scenario::InvalidManagedHeader,
            Scenario::InvalidNthHitZero,
            Scenario::InvalidTimeoutZero,
            Scenario::InvalidContentLengthDeltaZero,
            Scenario::InvalidTerminalCombination,
            Scenario::InvalidShiftJis,
        ] {
            assert!(scenario.is_invalid_config(), "{}", scenario.name());
            assert_eq!(scenario.expected_hit_count(), 0);
        }
    }

    #[test]
    fn reports_primary_and_cleanup_failures_together() {
        let error = finish_run(
            Some("request failed".to_owned()),
            vec!["delete rule failed".to_owned()],
            None,
        )
        .expect_err("both failures must be reported")
        .to_string();

        assert!(error.contains("scenario failed: request failed"));
        assert!(error.contains("emergency rule cleanup failed: delete rule failed"));
    }

    #[test]
    fn reports_primary_and_shutdown_failures_together() {
        let error = finish_run(
            Some("request failed".to_owned()),
            Vec::new(),
            Some("listener still running".to_owned()),
        )
        .expect_err("both failures must be reported")
        .to_string();

        assert!(error.contains("scenario failed: request failed"));
        assert!(error.contains("host shutdown failed: listener still running"));
    }
}
