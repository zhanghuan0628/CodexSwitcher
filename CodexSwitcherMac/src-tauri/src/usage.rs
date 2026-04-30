use chrono::{Duration, Local, TimeZone};
use reqwest::blocking::Client;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{error::Error, time::Duration as StdDuration};

const REAL_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const REAL_USAGE_MAX_ATTEMPTS: usize = 2;
const REAL_USAGE_RETRY_DELAY_MS: u64 = 250;
const REAL_USAGE_TIMEOUT_SECS: u64 = 6;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountStatus {
    Healthy,
    Warning,
    Exhausted,
    AuthInvalid,
    Error,
}

impl AccountStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Warning => "warning",
            Self::Exhausted => "exhausted",
            Self::AuthInvalid => "auth_invalid",
            Self::Error => "error",
        }
    }

    fn from_percent(percent: i64) -> Self {
        if percent >= 100 {
            Self::Exhausted
        } else if percent >= 85 {
            Self::Warning
        } else {
            Self::Healthy
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct UsageSnapshot {
    pub(crate) account_id: i64,
    pub(crate) sample_time: String,
    pub(crate) window_5h_percent: i64,
    pub(crate) window_7d_percent: i64,
    pub(crate) risk_level: String,
    pub(crate) estimated_reset_5h_at: Option<String>,
    pub(crate) estimated_reset_7d_at: Option<String>,
    pub(crate) source_type: String,
    pub(crate) confidence_level: String,
    pub(crate) is_estimated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RealUsageReading {
    pub(crate) window_5h_percent: i64,
    pub(crate) window_7d_percent: i64,
    pub(crate) confidence_label: String,
    pub(crate) estimated_reset_5h_at: Option<String>,
    pub(crate) estimated_reset_7d_at: Option<String>,
    pub(crate) raw_meta_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RealUsageReadErrorKind {
    AuthInvalid,
    Retryable,
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct RealUsageReadError {
    pub(crate) kind: RealUsageReadErrorKind,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize)]
struct StoredCredentials {
    #[serde(default)]
    tokens: Option<StoredCredentialTokens>,
}

#[derive(Debug, Deserialize)]
struct StoredCredentialTokens {
    #[serde(default)]
    access_token: Option<String>,
}

#[derive(Debug)]
enum RealUsageReadState {
    Ready(String),
    MissingToken,
}

#[derive(Debug, Deserialize)]
struct WhamUsageEnvelope {
    #[serde(default)]
    rate_limit: Option<WhamRateLimit>,
}

#[derive(Debug, Deserialize)]
struct WhamRateLimit {
    #[serde(default)]
    primary_window: Option<WhamUsageWindow>,
    #[serde(default)]
    secondary_window: Option<WhamUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct WhamUsageWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>,
}

pub(crate) fn usage_risk_from_windows(
    window_5h_percent: i64,
    window_7d_percent: i64,
) -> AccountStatus {
    AccountStatus::from_percent(window_5h_percent.max(window_7d_percent))
}

fn parse_real_usage_read_state(credentials_json: &str) -> Result<RealUsageReadState, String> {
    let credentials: StoredCredentials = serde_json::from_str(credentials_json)
        .map_err(|error| format!("解析账号凭证失败：{}", error))?;

    let Some(token) = credentials
        .tokens
        .and_then(|tokens| tokens.access_token)
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
    else {
        return Ok(RealUsageReadState::MissingToken);
    };

    Ok(RealUsageReadState::Ready(token))
}

fn round_percent(value: Option<f64>) -> i64 {
    value.unwrap_or(0.0).round().clamp(0.0, 100.0) as i64
}

fn unix_seconds_to_local_text(value: Option<i64>) -> Option<String> {
    value.and_then(|seconds| {
        Local
            .timestamp_opt(seconds, 0)
            .single()
            .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
    })
}

fn build_real_usage_meta_json(
    account_id: i64,
    profile_ref: Option<&str>,
    payload: &Value,
) -> String {
    serde_json::json!({
        "kind": "real_usage_refresh",
        "account_id": account_id,
        "endpoint": REAL_USAGE_ENDPOINT,
        "profile_ref": profile_ref,
        "payload": payload,
    })
    .to_string()
}

fn error_chain_text(error: &(dyn Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut current = Some(error);

    while let Some(item) = current {
        let text = item.to_string();
        if !text.trim().is_empty() && parts.last() != Some(&text) {
            parts.push(text);
        }
        current = item.source();
    }

    parts.join(" <- ")
}

fn classify_real_usage_status(status: reqwest::StatusCode) -> RealUsageReadErrorKind {
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        RealUsageReadErrorKind::AuthInvalid
    } else if status.is_server_error() {
        RealUsageReadErrorKind::Retryable
    } else {
        RealUsageReadErrorKind::Other
    }
}

fn should_retry_real_usage_failure(
    kind: &RealUsageReadErrorKind,
    attempt: usize,
    max_attempts: usize,
) -> bool {
    *kind == RealUsageReadErrorKind::Retryable && attempt < max_attempts
}

fn format_transport_error(error: &reqwest::Error) -> RealUsageReadError {
    let category = if error.is_timeout() {
        "请求真实额度接口超时"
    } else if error.is_connect() {
        "连接真实额度接口失败"
    } else {
        "请求真实额度接口失败"
    };

    let kind = if error.is_timeout() || error.is_connect() {
        RealUsageReadErrorKind::Retryable
    } else {
        RealUsageReadErrorKind::Other
    };

    RealUsageReadError {
        kind,
        message: format!("{}：{}", category, error_chain_text(error)),
    }
}

fn format_status_error(status: reqwest::StatusCode, body: &str) -> RealUsageReadError {
    let summary = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect::<String>();
    let message = if summary.is_empty() {
        format!("真实额度接口返回状态 {}", status)
    } else {
        format!("真实额度接口返回状态 {}：{}", status, summary)
    };

    RealUsageReadError {
        kind: classify_real_usage_status(status),
        message,
    }
}

fn fetch_real_usage_payload_once(
    client: &Client,
    profile_ref: Option<&str>,
    access_token: &str,
) -> Result<Value, RealUsageReadError> {
    let mut request = client
        .get(REAL_USAGE_ENDPOINT)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/json");

    if let Some(profile_ref) = profile_ref {
        let trimmed = profile_ref.trim();
        if !trimmed.is_empty() {
            request = request.header("ChatGPT-Account-ID", trimmed);
        }
    }

    let response = request
        .send()
        .map_err(|error| format_transport_error(&error))?;
    let status = response.status();
    let body = response.text().unwrap_or_default();

    if !status.is_success() {
        return Err(format_status_error(status, &body));
    }

    serde_json::from_str(&body).map_err(|error| RealUsageReadError {
        kind: RealUsageReadErrorKind::Other,
        message: format!("解析真实额度响应失败：{}", error),
    })
}

fn fetch_real_usage_payload_from_credentials(
    profile_ref: Option<&str>,
    credentials_json: &str,
) -> Result<Option<Value>, RealUsageReadError> {
    let access_token = match parse_real_usage_read_state(credentials_json).map_err(|message| {
        RealUsageReadError {
            kind: RealUsageReadErrorKind::Other,
            message,
        }
    })? {
        RealUsageReadState::Ready(token) => token,
        RealUsageReadState::MissingToken => return Ok(None),
    };
    let client = Client::builder()
        .timeout(StdDuration::from_secs(REAL_USAGE_TIMEOUT_SECS))
        .user_agent("CodexSwitcherMac/0.1")
        .build()
        .map_err(|error| RealUsageReadError {
            kind: RealUsageReadErrorKind::Other,
            message: format!("创建真实额度请求客户端失败：{}", error),
        })?;

    let mut last_error = None;

    for attempt in 1..=REAL_USAGE_MAX_ATTEMPTS {
        match fetch_real_usage_payload_once(&client, profile_ref, &access_token) {
            Ok(payload) => return Ok(Some(payload)),
            Err(error) => {
                if should_retry_real_usage_failure(&error.kind, attempt, REAL_USAGE_MAX_ATTEMPTS) {
                    last_error = Some(error);
                    std::thread::sleep(StdDuration::from_millis(REAL_USAGE_RETRY_DELAY_MS));
                    continue;
                }

                return Err(if attempt > 1 {
                    RealUsageReadError {
                        kind: error.kind,
                        message: format!("{}（已重试 {} 次）", error.message, attempt - 1),
                    }
                } else {
                    error
                });
            }
        }
    }

    let error = last_error.expect("retry loop should keep the last error");
    Err(RealUsageReadError {
        kind: error.kind,
        message: format!(
            "{}（已重试 {} 次）",
            error.message,
            REAL_USAGE_MAX_ATTEMPTS - 1
        ),
    })
}

pub(crate) fn read_real_usage_from_credentials(
    account_id: i64,
    profile_ref: Option<&str>,
    credentials_json: &str,
) -> Result<Option<RealUsageReading>, RealUsageReadError> {
    let Some(payload) = fetch_real_usage_payload_from_credentials(profile_ref, credentials_json)?
    else {
        return Ok(None);
    };
    let envelope: WhamUsageEnvelope =
        serde_json::from_value(payload.clone()).map_err(|error| RealUsageReadError {
            kind: RealUsageReadErrorKind::Other,
            message: format!("解析真实额度结构失败：{}", error),
        })?;
    let Some(rate_limit) = envelope.rate_limit else {
        return Ok(None);
    };
    let Some(primary) = rate_limit.primary_window else {
        return Ok(None);
    };

    let window_5h_percent = round_percent(primary.used_percent);
    let (window_7d_percent, estimated_reset_7d_at) =
        if let Some(secondary) = rate_limit.secondary_window {
            (
                round_percent(secondary.used_percent),
                unix_seconds_to_local_text(secondary.reset_at),
            )
        } else {
            (0, None)
        };

    Ok(Some(RealUsageReading {
        window_5h_percent,
        window_7d_percent,
        confidence_label: "精确".to_string(),
        estimated_reset_5h_at: unix_seconds_to_local_text(primary.reset_at),
        estimated_reset_7d_at,
        raw_meta_json: build_real_usage_meta_json(account_id, profile_ref, &payload),
    }))
}

pub(crate) fn insert_real_usage_snapshot(
    connection: &Connection,
    account_id: i64,
    sample_at: chrono::DateTime<Local>,
    reading: &RealUsageReading,
) -> Result<(), String> {
    let risk = usage_risk_from_windows(reading.window_5h_percent, reading.window_7d_percent)
        .as_str()
        .to_string();
    let estimated_5h = reading.estimated_reset_5h_at.clone().unwrap_or_else(|| {
        (sample_at + Duration::minutes((100 - reading.window_5h_percent).max(1)))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    });
    let estimated_7d = reading.estimated_reset_7d_at.clone().unwrap_or_else(|| {
        (sample_at + Duration::hours(((100 - reading.window_7d_percent) / 4).max(2)))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    });
    let sample_text = sample_at.format("%Y-%m-%d %H:%M:%S").to_string();

    connection
        .execute(
            "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'real_usage', ?8, 0, ?9)",
            params![
                account_id,
                sample_text,
                reading.window_5h_percent,
                reading.window_7d_percent,
                risk,
                estimated_5h,
                estimated_7d,
                reading.confidence_label,
                reading.raw_meta_json,
            ],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "UPDATE accounts SET status = ?1, auth_state = 'valid', last_verified_at = ?2, last_check_time = ?2, estimated_reset_time = ?3, updated_at = ?2 WHERE id = ?4",
            params![risk, sample_text, estimated_5h, account_id],
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

pub(crate) fn query_latest_snapshot(
    connection: &Connection,
    account_id: i64,
) -> Result<Option<UsageSnapshot>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated
             FROM usage_snapshots WHERE account_id = ?1 ORDER BY id DESC LIMIT 1",
        )
        .map_err(|error| error.to_string())?;

    let mut rows = stmt
        .query([account_id])
        .map_err(|error| error.to_string())?;
    if let Some(row) = rows.next().map_err(|error| error.to_string())? {
        Ok(Some(UsageSnapshot {
            account_id: row.get(0).map_err(|error| error.to_string())?,
            sample_time: row.get(1).map_err(|error| error.to_string())?,
            window_5h_percent: row.get(2).map_err(|error| error.to_string())?,
            window_7d_percent: row.get(3).map_err(|error| error.to_string())?,
            risk_level: row.get(4).map_err(|error| error.to_string())?,
            estimated_reset_5h_at: row.get(5).map_err(|error| error.to_string())?,
            estimated_reset_7d_at: row.get(6).map_err(|error| error.to_string())?,
            source_type: row.get(7).map_err(|error| error.to_string())?,
            confidence_level: row.get(8).map_err(|error| error.to_string())?,
            is_estimated: row.get::<_, i64>(9).map_err(|error| error.to_string())? == 1,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
pub(crate) fn query_latest_real_usage_snapshot(
    connection: &Connection,
    account_id: i64,
) -> Result<Option<UsageSnapshot>, String> {
    let latest = query_latest_snapshot(connection, account_id)?;
    Ok(latest.filter(|snapshot| snapshot.source_type == "real_usage" && !snapshot.is_estimated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    #[derive(Debug)]
    struct NestedTestError {
        message: &'static str,
        source: Option<Box<dyn Error + Send + Sync>>,
    }

    impl Display for NestedTestError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl Error for NestedTestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source
                .as_ref()
                .map(|source| source.as_ref() as &(dyn Error + 'static))
        }
    }

    #[test]
    fn error_chain_text_includes_nested_causes() {
        let root = NestedTestError {
            message: "connection reset by peer",
            source: None,
        };
        let middle = NestedTestError {
            message: "tcp connect error",
            source: Some(Box::new(root)),
        };
        let top = NestedTestError {
            message: "request send failed",
            source: Some(Box::new(middle)),
        };

        assert_eq!(
            error_chain_text(&top),
            "request send failed <- tcp connect error <- connection reset by peer"
        );
    }

    #[test]
    fn real_usage_status_classification_marks_auth_invalid_and_retryable_cases() {
        assert_eq!(
            classify_real_usage_status(StatusCode::FORBIDDEN),
            RealUsageReadErrorKind::AuthInvalid
        );
        assert_eq!(
            classify_real_usage_status(StatusCode::UNAUTHORIZED),
            RealUsageReadErrorKind::AuthInvalid
        );
        assert_eq!(
            classify_real_usage_status(StatusCode::SERVICE_UNAVAILABLE),
            RealUsageReadErrorKind::Retryable
        );
        assert_eq!(
            classify_real_usage_status(StatusCode::BAD_REQUEST),
            RealUsageReadErrorKind::Other
        );
    }

    #[test]
    fn real_usage_retry_policy_only_retries_retryable_failures_with_attempts_left() {
        assert!(should_retry_real_usage_failure(
            &RealUsageReadErrorKind::Retryable,
            1,
            3
        ));
        assert!(!should_retry_real_usage_failure(
            &RealUsageReadErrorKind::Retryable,
            3,
            3
        ));
        assert!(!should_retry_real_usage_failure(
            &RealUsageReadErrorKind::AuthInvalid,
            1,
            3
        ));
    }
}
