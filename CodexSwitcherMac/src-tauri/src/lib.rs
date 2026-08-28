mod auth;
mod keychain;
mod startup_health;
mod switching;
mod usage;

use auth::{
    codex_config_dir, diagnose_bind_environment, extract_codex_identity_from_session_json,
    read_codex_auth_status_cached, resolve_codex_cli_path,
    start_codex_login_flow as open_codex_login_flow, verify_real_codex_session,
    BindEnvironmentDiagnostic, CodexAuthStatus,
};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Timelike};
#[cfg(test)]
use keychain::decode_hex_payload_if_needed;
use keychain::{
    delete_account_secret, keychain_account_key, keychain_session_ref, load_account_secret,
    store_account_secret,
};
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use startup_health::{build_startup_health, StartupHealth};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Mutex,
    },
    thread,
    time::Duration as StdDuration,
};
#[cfg(not(test))]
use std::{io::Write, sync::mpsc, time::Instant};
use switching::{
    insert_switch_log, is_switchable, latest_switch_for_account, query_recent_switches_for_account,
    query_switch_logs, rollback_active_account, set_active_account,
};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use usage::{
    insert_real_usage_snapshot, lowest_known_remaining, query_latest_real_usage_snapshot,
    query_latest_snapshot, read_real_usage_from_credentials, usage_percent_text,
    RealUsageReadError, RealUsageReadErrorKind, RealUsageReading, UsageSnapshot,
};
use uuid::Uuid;

const TRAY_ID: &str = "codexswitcher-menu";
const RECOVERY_REMINDER_MINUTES: i64 = 15;
const DEFAULT_CHECK_INTERVAL_SECS: i64 = 60;

struct AppState {
    db: Mutex<Connection>,
    sampling_in_progress: AtomicBool,
}

struct SamplingRunGuard<'a> {
    state: &'a AppState,
}

impl Drop for SamplingRunGuard<'_> {
    fn drop(&mut self) {
        self.state
            .sampling_in_progress
            .store(false, AtomicOrdering::Release);
    }
}

fn try_begin_sampling_run(state: &AppState) -> Option<SamplingRunGuard<'_>> {
    state
        .sampling_in_progress
        .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
        .ok()?;
    Some(SamplingRunGuard { state })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Account {
    pub(crate) id: i64,
    pub(crate) provider: String,
    pub(crate) nickname: String,
    pub(crate) status: String,
    pub(crate) is_active: bool,
    pub(crate) is_default: bool,
    pub(crate) auth_state: String,
    pub(crate) last_check_time: Option<String>,
    pub(crate) estimated_reset_time: Option<String>,
    pub(crate) account_key: String,
    pub(crate) binding_kind: String,
    pub(crate) session_ref: String,
    pub(crate) profile_ref: Option<String>,
    pub(crate) account_email: Option<String>,
    pub(crate) last_verified_at: Option<String>,
    pub(crate) is_real_session: bool,
    pub(crate) plan_label: Option<String>,
    pub(crate) latest_snapshot: Option<UsageSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CredentialProfile {
    id: i64,
    profile_kind: String,
    provider: String,
    nickname: String,
    status: String,
    is_active: bool,
    base_url: Option<String>,
    model: Option<String>,
    masked_secret: Option<String>,
    secret_ref: Option<String>,
    linked_account_id: Option<i64>,
    usage_provider_type: Option<String>,
    usage_query_user: Option<String>,
    usage_query_app_version: Option<String>,
    usage_masked_secret: Option<String>,
    usage_secret_ref: Option<String>,
    usage_summary: Option<ThirdPartyKeyUsageSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ThirdPartyKeyUsageDetailItem {
    label: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ThirdPartyKeyUsageBucket {
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    cost: f64,
    actual_cost: f64,
    account_cost: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ThirdPartyKeyUsageModelStat {
    model: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    cost: f64,
    actual_cost: f64,
    account_cost: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ThirdPartyKeyUsageSummary {
    status: String,
    message: Option<String>,
    fetched_at: String,
    usage_endpoint: Option<String>,
    usage_provider_type: Option<String>,
    balance: Option<f64>,
    remaining: Option<f64>,
    unit: Option<String>,
    is_valid: Option<bool>,
    mode: Option<String>,
    plan_name: Option<String>,
    average_duration_ms: Option<f64>,
    rpm: Option<i64>,
    tpm: Option<i64>,
    today: Option<ThirdPartyKeyUsageBucket>,
    total: Option<ThirdPartyKeyUsageBucket>,
    model_stats: Vec<ThirdPartyKeyUsageModelStat>,
    detail_items: Vec<ThirdPartyKeyUsageDetailItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ThirdPartyKeyUsageApiResponse {
    balance: Option<f64>,
    remaining: Option<f64>,
    #[serde(rename = "isValid")]
    is_valid: Option<bool>,
    mode: Option<String>,
    #[serde(rename = "planName")]
    plan_name: Option<String>,
    unit: Option<String>,
    usage: ThirdPartyKeyUsageApiEnvelope,
    model_stats: Vec<ThirdPartyKeyUsageApiModelStat>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ThirdPartyKeyUsageApiEnvelope {
    average_duration_ms: Option<f64>,
    rpm: Option<i64>,
    today: Option<ThirdPartyKeyUsageApiBucket>,
    total: Option<ThirdPartyKeyUsageApiBucket>,
    tpm: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ThirdPartyKeyUsageApiBucket {
    requests: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cost: Option<f64>,
    actual_cost: Option<f64>,
    account_cost: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ThirdPartyKeyUsageApiModelStat {
    model: Option<String>,
    requests: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cost: Option<f64>,
    actual_cost: Option<f64>,
    account_cost: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct NewApiUserSelfResponse {
    success: Option<bool>,
    message: Option<String>,
    data: Option<NewApiUserSelfData>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct NewApiUserSelfData {
    id: Option<i64>,
    group: Option<String>,
    display_name: Option<String>,
    username: Option<String>,
    quota: Option<f64>,
    used_quota: Option<f64>,
    request_count: Option<i64>,
    status: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CreateKeyProfileInput {
    provider: String,
    nickname: String,
    base_url: String,
    model: String,
    api_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct UpdateKeyProfileInput {
    id: i64,
    provider: String,
    nickname: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct UpdateKeyProfileUsageConfigInput {
    profile_id: i64,
    usage_provider_type: Option<String>,
    usage_query_user: Option<String>,
    usage_query_app_version: Option<String>,
    usage_access_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppSettings {
    warn_threshold_low: i64,
    warn_threshold_mid: i64,
    warn_threshold_high: i64,
    check_interval: i64,
    enable_handoff: bool,
    prefer_official_upgrade: bool,
    enable_auto_refresh: bool,
    enable_auto_sampling: bool,
    foreground_auto_sampling_only: bool,
    launch_at_login: bool,
    menu_bar_only: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct SwitchLog {
    pub(crate) id: i64,
    pub(crate) from_account_id: Option<i64>,
    pub(crate) to_account_id: i64,
    pub(crate) result: String,
    pub(crate) reason: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LocalProject {
    id: i64,
    name: String,
    workspace_path: String,
    git_remote: Option<String>,
    last_active_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SessionRecord {
    id: i64,
    project_id: i64,
    project_name: String,
    project_path: String,
    owner_account_id: Option<i64>,
    owner_profile_kind: String,
    owner_profile_ref: String,
    record_type: String,
    title: String,
    summary: String,
    raw_content: String,
    message_count: i64,
    source_record_id: Option<i64>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NotificationItem {
    id: i64,
    account_id: Option<i64>,
    level: String,
    title: String,
    message: String,
    source_type: String,
    action_type: String,
    related_handoff_id: Option<i64>,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChartSeriesValue {
    account_id: i64,
    account_name: String,
    value: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChartPoint {
    label: String,
    series: Vec<ChartSeriesValue>,
    event_label: Option<String>,
    source_label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TimelineSegment {
    state: String,
    hours: i64,
    label: String,
    tooltip: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TimelineLane {
    account_id: i64,
    account_name: String,
    confidence: String,
    next_action: String,
    segments: Vec<TimelineSegment>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct UsageDisplayState {
    status: String,
    source_type: String,
    confidence_label: String,
    summary: String,
    helper_text: String,
    chart_helper_text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SamplingSummary {
    kind: String,
    message: String,
    source_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CurrentCodexLogin {
    logged_in: bool,
    email: Option<String>,
    account_id: Option<String>,
    is_bound: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DashboardOverview {
    active_account: Option<Account>,
    accounts: Vec<Account>,
    current_login: Option<CurrentCodexLogin>,
    latest_snapshot: Option<UsageSnapshot>,
    usage_display: UsageDisplayState,
    latest_sampling: SamplingSummary,
    chart_points: Vec<ChartPoint>,
    timeline: Vec<TimelineLane>,
    recommendations: Vec<String>,
    recommended_account_id: Option<i64>,
    recommended_reason: Option<String>,
    switch_logs: Vec<SwitchLog>,
    settings: AppSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BootstrapState {
    overview: DashboardOverview,
    accounts: Vec<Account>,
    settings: AppSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkspaceSupportData {
    projects: Vec<LocalProject>,
    sessions: Vec<SessionRecord>,
    notifications: Vec<NotificationItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CodexLocalSessionImportResult {
    scanned_files: i64,
    imported_sessions: i64,
    updated_sessions: i64,
    skipped_files: i64,
    codex_synced_threads: i64,
    codex_skipped_threads: i64,
    project_count: i64,
    session_count: i64,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CodexLocalSessionCandidate {
    candidate_id: String,
    identity_key: String,
    identity_label: String,
    identity_kind_label: String,
    project_name: String,
    project_path: String,
    title: String,
    message_count: i64,
    source_path: String,
    created_at: String,
    updated_at: String,
    imported_session_id: Option<i64>,
    imported_owner_profile_kind: Option<String>,
    imported_owner_profile_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AccountDiagnostic {
    account_id: i64,
    nickname: String,
    email: Option<String>,
    profile_ref: Option<String>,
    status: String,
    auth_state: String,
    keychain_readable: bool,
    latest_sample_at: Option<String>,
    latest_switch_at: Option<String>,
    advice: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ReleaseDiagnostic {
    generated_at: String,
    codex_cli_available: bool,
    codex_cli_path: Option<String>,
    current_login: Option<CurrentCodexLogin>,
    database_ok: bool,
    account_count: i64,
    latest_sampling: SamplingSummary,
    latest_switch: Option<SwitchLog>,
    accounts: Vec<AccountDiagnostic>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AccountDetail {
    account: Account,
    recent_snapshots: Vec<UsageSnapshot>,
    recent_switches: Vec<SwitchLog>,
    recent_notifications: Vec<NotificationItem>,
    recent_sessions: Vec<SessionRecord>,
    keychain_readable: bool,
    bound_snapshot_summary: Option<String>,
    last_failure_reason: Option<String>,
    health_timeline: Vec<TimelineSegment>,
    diagnostic_text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CleanupPreview {
    old_handoff_count: i64,
    old_notification_count: i64,
    orphan_handoff_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CleanupResult {
    old_handoff_count: i64,
    old_notification_count: i64,
    orphan_handoff_count: i64,
    deleted_total: i64,
}

#[derive(Debug, Deserialize)]
struct BindCurrentCodexAccountInput {
    nickname: String,
}

#[derive(Debug, Clone)]
struct SessionSnapshot {
    session_ref: String,
    credentials_json: String,
}

fn live_session_snapshot(status: &CodexAuthStatus) -> Result<SessionSnapshot, String> {
    let writable_session_ref = status.auth_path.clone().unwrap_or_else(|| {
        PathBuf::from(&status.config_dir)
            .join("auth.json")
            .to_string_lossy()
            .to_string()
    });

    if Path::new(&writable_session_ref).exists() {
        return read_session_snapshot(&writable_session_ref);
    }

    if status.session_json.trim().is_empty() {
        return Err("当前官方登录态内容为空，无法完成真实绑定。".to_string());
    }

    Ok(SessionSnapshot {
        session_ref: writable_session_ref,
        credentials_json: status.session_json.clone(),
    })
}

fn read_real_usage_from_live_session(
    account: &Account,
    snapshot: &SessionSnapshot,
) -> Result<Option<RealUsageReading>, RealUsageReadError> {
    read_real_usage_from_session_with_profile(account, snapshot, account.profile_ref.as_deref())
}

fn read_real_usage_from_active_live_session(
    account: &Account,
    snapshot: &SessionSnapshot,
) -> Result<Option<RealUsageReading>, RealUsageReadError> {
    read_real_usage_from_session_with_profile(account, snapshot, None)
}

fn read_real_usage_from_session_with_profile(
    account: &Account,
    snapshot: &SessionSnapshot,
    profile_ref: Option<&str>,
) -> Result<Option<RealUsageReading>, RealUsageReadError> {
    read_real_usage_from_credentials(account.id, profile_ref, &snapshot.credentials_json)
}

fn mark_account_auth_invalid(
    connection: &Connection,
    account_id: i64,
    checked_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE accounts
             SET status = 'auth_invalid', auth_state = 'expired', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1
             WHERE id = ?2",
            params![checked_at, account_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
fn apply_real_usage_read_error(
    connection: &Connection,
    account: &Account,
    checked_at: &str,
    error: RealUsageReadError,
) -> Result<bool, String> {
    if error.kind == RealUsageReadErrorKind::AuthInvalid {
        mark_account_auth_invalid(connection, account.id, checked_at)?;
    }

    Err(error.message)
}

fn now_text() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[tauri::command]
fn diagnose_bind_environment_command(
    state: State<'_, AppState>,
) -> Result<BindEnvironmentDiagnostic, String> {
    let mut diagnostic = diagnose_bind_environment()?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let accounts = query_accounts(&connection)?;
    diagnostic.current_is_bound = accounts.iter().any(|account| {
        diagnostic
            .current_email
            .as_deref()
            .is_some_and(|email| account.account_email.as_deref() == Some(email))
            || diagnostic
                .current_account_id
                .as_deref()
                .is_some_and(|account_id| account.profile_ref.as_deref() == Some(account_id))
    });
    Ok(diagnostic)
}

#[tauri::command]
fn start_codex_login_flow() -> Result<String, String> {
    open_codex_login_flow()
}

fn delete_session_snapshot(session_ref: &str) -> Result<(), String> {
    if let Some(account_key) = keychain_account_key(session_ref) {
        return delete_account_secret(account_key);
    }

    let snapshot_path = Path::new(session_ref);
    if snapshot_path.exists() {
        fs::remove_file(snapshot_path).map_err(|error| format!("删除账号快照失败：{}", error))?;
    }
    Ok(())
}

fn read_session_snapshot(session_ref: &str) -> Result<SessionSnapshot, String> {
    let credentials_json = if let Some(account_key) = keychain_account_key(session_ref) {
        load_account_secret(account_key)?
    } else {
        let path = Path::new(session_ref);
        if !path.exists() {
            return Err(format!("会话文件不存在：{}", session_ref));
        }

        fs::read_to_string(path).map_err(|error| format!("读取会话文件失败：{}", error))?
    };

    if credentials_json.trim().is_empty() {
        return Err("当前会话内容为空，无法执行真实切换。".to_string());
    }

    Ok(SessionSnapshot {
        session_ref: session_ref.to_string(),
        credentials_json,
    })
}

fn read_bound_session_snapshot(account: &Account) -> Result<SessionSnapshot, String> {
    match read_session_snapshot(&account.session_ref) {
        Ok(snapshot) => Ok(snapshot),
        Err(session_error) => {
            let fallback_key = account.account_key.trim();
            if fallback_key.is_empty() {
                return Err(session_error);
            }

            let fallback_ref = keychain_session_ref(fallback_key);
            match read_session_snapshot(&fallback_ref) {
                Ok(snapshot) => Ok(snapshot),
                Err(_) => Err(session_error),
            }
        }
    }
}

fn account_matches_verified_identity(account: &Account, verified: &CodexAuthStatus) -> bool {
    account
        .profile_ref
        .as_deref()
        .is_some_and(|account_id| verified.account_id.as_deref() == Some(account_id))
        || account
            .account_email
            .as_deref()
            .is_some_and(|email| verified.account_email.as_deref() == Some(email))
}

fn ensure_verify_target_matches_current_login(
    account: &Account,
    verified: &CodexAuthStatus,
) -> Result<(), String> {
    if account_matches_verified_identity(account, verified) {
        return Ok(());
    }

    let expected = account
        .account_email
        .as_deref()
        .or(account.profile_ref.as_deref())
        .unwrap_or("该账号");
    let current = verified
        .account_email
        .as_deref()
        .or(verified.account_id.as_deref())
        .unwrap_or("未知官方登录态");
    Err(format!(
        "当前官方登录态是 {}，不是 {}。请先切到该账号或使用“重新登录并重绑”。",
        current, expected
    ))
}

fn ensure_account_snapshot_from_live_session(
    account: &Account,
    verified: &CodexAuthStatus,
    live_snapshot: &SessionSnapshot,
) -> Result<SessionSnapshot, String> {
    match read_bound_session_snapshot(account) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            if !account_matches_verified_identity(account, verified) {
                return Err(error);
            }

            let session_ref = if account.session_ref.trim().is_empty()
                || keychain_account_key(&account.session_ref).is_some()
            {
                let account_key = account.account_key.trim();
                if account_key.is_empty() {
                    return Err(error);
                }
                store_account_secret(account_key, &live_snapshot.credentials_json)?
            } else {
                restore_session_snapshot(&SessionSnapshot {
                    session_ref: account.session_ref.clone(),
                    credentials_json: live_snapshot.credentials_json.clone(),
                })?;
                account.session_ref.clone()
            };

            Ok(SessionSnapshot {
                session_ref,
                credentials_json: live_snapshot.credentials_json.clone(),
            })
        }
    }
}

fn restore_session_snapshot(snapshot: &SessionSnapshot) -> Result<(), String> {
    if let Some(account_key) = keychain_account_key(&snapshot.session_ref) {
        store_account_secret(account_key, &snapshot.credentials_json).map(|_| ())
    } else {
        fs::write(&snapshot.session_ref, &snapshot.credentials_json)
            .map_err(|error| format!("恢复会话文件失败：{}", error))
    }
}

fn migrate_legacy_session_ref_for_account(
    connection: &Connection,
    account_id: i64,
    account_key: &str,
    session_ref: &str,
) -> Result<(), String> {
    if account_key.trim().is_empty()
        || session_ref.trim().is_empty()
        || keychain_account_key(session_ref).is_some()
    {
        return Ok(());
    }

    let snapshot_path = Path::new(session_ref);
    if !snapshot_path.exists() {
        return Ok(());
    }

    let snapshot = read_session_snapshot(session_ref)?;
    let migrated_session_ref = match store_account_secret(account_key, &snapshot.credentials_json) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    connection
        .execute(
            "UPDATE accounts SET session_ref = ?1, updated_at = ?2 WHERE id = ?3",
            params![migrated_session_ref, now_text(), account_id],
        )
        .map_err(|error| error.to_string())?;
    fs::remove_file(snapshot_path).map_err(|error| format!("删除旧会话文件失败：{}", error))?;
    Ok(())
}

fn migrate_legacy_session_refs(connection: &Connection) -> Result<(), String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, account_key, session_ref
             FROM accounts
             WHERE is_real_session = 1 AND binding_kind = 'codex_cli' AND session_ref != ''",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let accounts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(stmt);

    for (account_id, account_key, session_ref) in accounts {
        migrate_legacy_session_ref_for_account(connection, account_id, &account_key, &session_ref)?;
    }

    Ok(())
}

fn reconcile_real_account_identity_fields(connection: &Connection) -> Result<(), String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, account_key, session_ref, profile_ref, account_email
             FROM accounts
             WHERE is_real_session = 1 AND binding_kind = 'codex_cli'",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let accounts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(stmt);

    for (account_id, account_key, session_ref, profile_ref, account_email) in accounts {
        let expected_profile_ref = expected_profile_ref_from_account_key(&account_key);

        if let Ok(snapshot) = read_session_snapshot(&session_ref) {
            let (next_profile_ref, next_account_email) =
                extract_codex_identity_from_session_json(&snapshot.credentials_json);

            if next_profile_ref.is_none() && next_account_email.is_none() {
                continue;
            }

            if expected_profile_ref.is_some()
                && next_profile_ref.is_some()
                && next_profile_ref != expected_profile_ref
            {
                connection
                    .execute(
                        "UPDATE accounts
                         SET profile_ref = ?1,
                             account_email = NULL,
                             auth_state = 'mismatch',
                             status = 'warning',
                             updated_at = ?2
                         WHERE id = ?3",
                        params![expected_profile_ref, now_text(), account_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            }

            if next_profile_ref == profile_ref && next_account_email == account_email {
                continue;
            }

            connection
                .execute(
                    "UPDATE accounts
                     SET profile_ref = COALESCE(?1, profile_ref),
                         account_email = COALESCE(?2, account_email),
                         updated_at = ?3
                     WHERE id = ?4",
                    params![next_profile_ref, next_account_email, now_text(), account_id],
                )
                .map_err(|error| error.to_string())?;
            continue;
        }

        let Some(expected_profile_ref) = expected_profile_ref else {
            continue;
        };

        if profile_ref.as_deref() == Some(expected_profile_ref.as_str()) {
            continue;
        }

        connection
            .execute(
                "UPDATE accounts
                 SET profile_ref = ?1,
                     account_email = CASE
                        WHEN account_email IS NOT NULL AND account_email != '' THEN NULL
                        ELSE account_email
                     END,
                     updated_at = ?2
                 WHERE id = ?3",
                params![expected_profile_ref, now_text(), account_id],
            )
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn sync_real_account_storage(connection: &Connection) -> Result<(), String> {
    migrate_legacy_session_refs(connection)?;
    reconcile_real_account_identity_fields(connection)?;
    Ok(())
}

fn sync_real_account_snapshot(
    account: &Account,
    live_snapshot: &SessionSnapshot,
) -> Result<(), String> {
    if !account.is_real_session || account.binding_kind != "codex_cli" {
        return Ok(());
    }

    let account_snapshot = SessionSnapshot {
        session_ref: account.session_ref.clone(),
        credentials_json: live_snapshot.credentials_json.clone(),
    };
    restore_session_snapshot(&account_snapshot)
}

fn restore_real_session_snapshot(
    target_snapshot: &SessionSnapshot,
    live_snapshot: &SessionSnapshot,
) -> Result<SessionSnapshot, String> {
    let restored_live_snapshot = SessionSnapshot {
        session_ref: live_snapshot.session_ref.clone(),
        credentials_json: target_snapshot.credentials_json.clone(),
    };
    restore_session_snapshot(&restored_live_snapshot)?;
    Ok(restored_live_snapshot)
}

fn sync_real_account_snapshot_in_background(account: Account, live_snapshot: SessionSnapshot) {
    thread::spawn(move || {
        let _ = sync_real_account_snapshot(&account, &live_snapshot);
    });
}

fn account_matches_live_session(
    account: &Account,
    verified: &CodexAuthStatus,
    live_snapshot: &SessionSnapshot,
) -> Result<bool, String> {
    let account_snapshot =
        ensure_account_snapshot_from_live_session(account, verified, live_snapshot)?;
    Ok(account_snapshot.credentials_json == live_snapshot.credentials_json)
}

fn bound_snapshot_matches_account(account: &Account, snapshot: &SessionSnapshot) -> bool {
    let (snapshot_profile_ref, snapshot_email) =
        extract_codex_identity_from_session_json(&snapshot.credentials_json);

    if snapshot_profile_ref.is_none() && snapshot_email.is_none() {
        return false;
    }

    if let Some(expected_profile_ref) = expected_profile_ref_from_account_key(&account.account_key)
    {
        if snapshot_profile_ref.as_deref() != Some(expected_profile_ref.as_str()) {
            return false;
        }
    }

    if let Some(profile_ref) = account.profile_ref.as_deref() {
        if let Some(snapshot_profile_ref) = snapshot_profile_ref.as_deref() {
            if snapshot_profile_ref != profile_ref {
                return false;
            }
        }
    }

    if let Some(account_email) = account.account_email.as_deref() {
        if let Some(snapshot_email) = snapshot_email.as_deref() {
            if snapshot_email != account_email {
                return false;
            }
        }
    }

    true
}

fn normalize_account_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == ' ' || ch == '-' || ch == '_' {
            out.push('-');
        }
    }

    while out.contains("--") {
        out = out.replace("--", "-");
    }

    out.trim_matches('-').to_string()
}

fn expected_profile_ref_from_account_key(account_key: &str) -> Option<String> {
    let value = account_key.strip_prefix("codex-")?.trim();
    if value.len() == 36 && value.chars().filter(|ch| *ch == '-').count() == 4 {
        Some(value.to_string())
    } else {
        None
    }
}

fn init_database(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                nickname TEXT NOT NULL,
                status TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0,
                is_default INTEGER NOT NULL DEFAULT 0,
                auth_state TEXT NOT NULL DEFAULT 'valid',
                last_check_time TEXT,
                estimated_reset_time TEXT,
                account_key TEXT NOT NULL DEFAULT '',
                binding_kind TEXT NOT NULL DEFAULT 'manual',
                session_ref TEXT NOT NULL DEFAULT '',
                profile_ref TEXT,
                account_email TEXT,
                last_verified_at TEXT,
                is_real_session INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                sample_time TEXT NOT NULL,
                window_5h_percent INTEGER NOT NULL,
                window_7d_percent INTEGER NOT NULL,
                risk_level TEXT NOT NULL,
                estimated_reset_5h_at TEXT,
                estimated_reset_7d_at TEXT,
                source_type TEXT NOT NULL,
                confidence_level TEXT NOT NULL,
                is_estimated INTEGER NOT NULL,
                raw_meta_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS switch_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_account_id INTEGER,
                to_account_id INTEGER NOT NULL,
                result TEXT NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS handoff_cards (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                task_title TEXT,
                goal TEXT,
                done_summary TEXT,
                todo_summary TEXT,
                changed_files TEXT,
                recent_commands TEXT,
                suggested_prompt TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS local_projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                workspace_path TEXT NOT NULL,
                git_remote TEXT,
                last_active_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                owner_account_id INTEGER,
                owner_profile_kind TEXT NOT NULL,
                owner_profile_ref TEXT NOT NULL,
                record_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                raw_content TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                source_record_id INTEGER,
                external_session_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_profile_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL,
                profile_kind TEXT NOT NULL,
                profile_ref TEXT NOT NULL,
                access_mode TEXT NOT NULL,
                source_session_id INTEGER,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS credential_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_kind TEXT NOT NULL,
                provider TEXT NOT NULL,
                nickname TEXT NOT NULL,
                status TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0,
                base_url TEXT,
                model TEXT,
                masked_secret TEXT,
                secret_ref TEXT,
                linked_account_id INTEGER,
                usage_provider_type TEXT,
                usage_query_user TEXT,
                usage_query_app_version TEXT,
                usage_masked_secret TEXT,
                usage_secret_ref TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER,
                level TEXT NOT NULL,
                title TEXT NOT NULL,
                message TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'system_event',
                action_type TEXT NOT NULL DEFAULT 'system_event',
                related_handoff_id INTEGER,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                warn_threshold_low INTEGER NOT NULL,
                warn_threshold_mid INTEGER NOT NULL,
                warn_threshold_high INTEGER NOT NULL,
                check_interval INTEGER NOT NULL,
                enable_handoff INTEGER NOT NULL,
                prefer_official_upgrade INTEGER NOT NULL,
                enable_auto_refresh INTEGER NOT NULL DEFAULT 1,
                enable_auto_sampling INTEGER NOT NULL DEFAULT 1,
                foreground_auto_sampling_only INTEGER NOT NULL DEFAULT 0,
                launch_at_login INTEGER NOT NULL,
                menu_bar_only INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS schema_migrations (
                name TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS codex_visibility_archives (
                thread_id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )
        .map_err(|error| error.to_string())?;

    migrate_accounts_schema(connection)?;
    seed_defaults(connection)?;
    migrate_real_usage_snapshots_to_remaining_percent(connection)?;
    cleanup_legacy_demo_data(connection)?;
    purge_legacy_handoff_cards(connection)?;
    sync_account_credential_profiles(connection)?;
    Ok(())
}

fn migrate_real_usage_snapshots_to_remaining_percent(
    connection: &Connection,
) -> Result<(), String> {
    let already_applied = connection
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE name = 'real_usage_remaining_percent_v1'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .is_some();

    if already_applied {
        return Ok(());
    }

    connection
        .execute(
            "UPDATE usage_snapshots
             SET window_5h_percent = MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.primary_window.used_percent') AS INTEGER))),
                 window_7d_percent = MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.secondary_window.used_percent') AS INTEGER))),
                 risk_level = CASE
                     WHEN MIN(
                         MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.primary_window.used_percent') AS INTEGER))),
                         MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.secondary_window.used_percent') AS INTEGER)))
                     ) <= 0 THEN 'exhausted'
                     WHEN MIN(
                         MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.primary_window.used_percent') AS INTEGER))),
                         MAX(0, MIN(100, 100 - CAST(json_extract(raw_meta_json, '$.payload.rate_limit.secondary_window.used_percent') AS INTEGER)))
                     ) <= 15 THEN 'warning'
                     ELSE 'healthy'
                 END
             WHERE source_type = 'real_usage'
               AND confidence_level = '精确'
               AND is_estimated = 0
               AND json_type(raw_meta_json, '$.payload.rate_limit.primary_window.used_percent') IN ('integer', 'real')
               AND json_type(raw_meta_json, '$.payload.rate_limit.secondary_window.used_percent') IN ('integer', 'real')",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "INSERT INTO schema_migrations (name, applied_at) VALUES ('real_usage_remaining_percent_v1', ?1)",
            [now_text()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_accounts_schema(connection: &Connection) -> Result<(), String> {
    let columns = [
        (
            "account_key",
            "ALTER TABLE accounts ADD COLUMN account_key TEXT NOT NULL DEFAULT ''",
        ),
        (
            "binding_kind",
            "ALTER TABLE accounts ADD COLUMN binding_kind TEXT NOT NULL DEFAULT 'manual'",
        ),
        (
            "session_ref",
            "ALTER TABLE accounts ADD COLUMN session_ref TEXT NOT NULL DEFAULT ''",
        ),
        (
            "profile_ref",
            "ALTER TABLE accounts ADD COLUMN profile_ref TEXT",
        ),
        (
            "account_email",
            "ALTER TABLE accounts ADD COLUMN account_email TEXT",
        ),
        (
            "last_verified_at",
            "ALTER TABLE accounts ADD COLUMN last_verified_at TEXT",
        ),
        (
            "is_real_session",
            "ALTER TABLE accounts ADD COLUMN is_real_session INTEGER NOT NULL DEFAULT 0",
        ),
    ];

    for (column, sql) in columns {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('accounts') WHERE name = ?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;

        if exists == 0 {
            connection
                .execute(sql, [])
                .map_err(|error| error.to_string())?;
        }
    }

    connection
        .execute(
            "UPDATE accounts
             SET account_key = CASE
                 WHEN account_key = '' THEN lower(replace(provider || '-' || nickname || '-' || id, ' ', '-'))
                 ELSE account_key
             END,
             binding_kind = CASE
                 WHEN binding_kind = '' THEN 'manual'
                 WHEN binding_kind = 'official_cli' THEN 'codex_cli'
                 ELSE binding_kind
             END,
             session_ref = COALESCE(session_ref, ''),
             is_real_session = COALESCE(is_real_session, 0)",
            [],
        )
        .map_err(|error| error.to_string())?;

    let notification_columns = [
        (
            "source_type",
            "ALTER TABLE notifications ADD COLUMN source_type TEXT NOT NULL DEFAULT 'system_event'",
        ),
        (
            "account_id",
            "ALTER TABLE notifications ADD COLUMN account_id INTEGER",
        ),
        (
            "action_type",
            "ALTER TABLE notifications ADD COLUMN action_type TEXT NOT NULL DEFAULT 'system_event'",
        ),
        (
            "related_handoff_id",
            "ALTER TABLE notifications ADD COLUMN related_handoff_id INTEGER",
        ),
    ];

    for (column, sql) in notification_columns {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('notifications') WHERE name = ?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;

        if exists == 0 {
            connection
                .execute(sql, [])
                .map_err(|error| error.to_string())?;
        }
    }

    let session_record_columns = [(
        "external_session_id",
        "ALTER TABLE session_records ADD COLUMN external_session_id TEXT",
    )];

    for (column, sql) in session_record_columns {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_records') WHERE name = ?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;

        if exists == 0 {
            connection
                .execute(sql, [])
                .map_err(|error| error.to_string())?;
        }
    }

    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_session_records_external_session
             ON session_records(record_type, external_session_id)",
            [],
        )
        .map_err(|error| error.to_string())?;

    let settings_columns = [
        (
            "enable_auto_refresh",
            "ALTER TABLE app_settings ADD COLUMN enable_auto_refresh INTEGER NOT NULL DEFAULT 1",
        ),
        (
            "enable_auto_sampling",
            "ALTER TABLE app_settings ADD COLUMN enable_auto_sampling INTEGER NOT NULL DEFAULT 1",
        ),
        (
            "foreground_auto_sampling_only",
            "ALTER TABLE app_settings ADD COLUMN foreground_auto_sampling_only INTEGER NOT NULL DEFAULT 0",
        ),
    ];

    for (column, sql) in settings_columns {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('app_settings') WHERE name = ?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;

        if exists == 0 {
            connection
                .execute(sql, [])
                .map_err(|error| error.to_string())?;
        }
    }

    let credential_profile_columns = [
        (
            "usage_provider_type",
            "ALTER TABLE credential_profiles ADD COLUMN usage_provider_type TEXT",
        ),
        (
            "usage_query_user",
            "ALTER TABLE credential_profiles ADD COLUMN usage_query_user TEXT",
        ),
        (
            "usage_query_app_version",
            "ALTER TABLE credential_profiles ADD COLUMN usage_query_app_version TEXT",
        ),
        (
            "usage_masked_secret",
            "ALTER TABLE credential_profiles ADD COLUMN usage_masked_secret TEXT",
        ),
        (
            "usage_secret_ref",
            "ALTER TABLE credential_profiles ADD COLUMN usage_secret_ref TEXT",
        ),
    ];

    for (column, sql) in credential_profile_columns {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('credential_profiles') WHERE name = ?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;

        if exists == 0 {
            connection
                .execute(sql, [])
                .map_err(|error| error.to_string())?;
        }
    }

    connection
        .execute(
            "UPDATE notifications
             SET action_type = CASE
                 WHEN action_type IS NULL OR action_type = '' THEN source_type
                 ELSE action_type
             END",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "UPDATE app_settings
             SET foreground_auto_sampling_only = 0
             WHERE id = 1 AND foreground_auto_sampling_only = 1",
            [],
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn cleanup_legacy_demo_data(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM usage_snapshots WHERE source_type != 'real_usage' OR is_estimated = 1",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "DELETE FROM notifications WHERE source_type = 'mock_estimator'",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "DELETE FROM notifications WHERE source_type = 'settings_event' AND title = '官方扩容优先'",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "DELETE FROM handoff_cards WHERE task_title = '继续收尾 Day 8'",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute("DELETE FROM handoff_cards WHERE account_id NOT IN (SELECT id FROM accounts WHERE is_real_session = 1)", [])
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "DELETE FROM accounts WHERE is_real_session = 0 OR binding_kind = 'manual'",
            [],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute("UPDATE accounts SET is_active = 0 WHERE is_real_session = 0 OR binding_kind = 'manual'", [])
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn purge_legacy_handoff_cards(connection: &Connection) -> Result<(), String> {
    connection
        .execute("DELETE FROM handoff_cards", [])
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM session_profile_links
             WHERE session_id IN (SELECT id FROM session_records WHERE record_type = 'legacy_handoff')",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM session_records WHERE record_type = 'legacy_handoff'",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM local_projects WHERE workspace_path = 'legacy://handoff-cards'",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM notifications WHERE action_type IN ('create_handoff', 'switch_handoff_created') OR source_type = 'handoff'",
            [],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn seed_defaults(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO app_settings (id, warn_threshold_low, warn_threshold_mid, warn_threshold_high, check_interval, enable_handoff, prefer_official_upgrade, enable_auto_refresh, enable_auto_sampling, foreground_auto_sampling_only, launch_at_login, menu_bar_only)
             VALUES (1, 70, 85, 95, ?1, 1, 1, 1, 1, 0, 0, 0)",
            [DEFAULT_CHECK_INTERVAL_SECS],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "UPDATE app_settings SET check_interval = ?1 WHERE id = 1 AND check_interval IN (15, 120)",
            [DEFAULT_CHECK_INTERVAL_SECS],
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "UPDATE app_settings SET check_interval = ?1 WHERE id = 1 AND check_interval < 10",
            [DEFAULT_CHECK_INTERVAL_SECS],
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(test)]
fn read_real_usage_for_bound_account(
    _connection: &Connection,
    account: &Account,
) -> Result<Option<RealUsageReading>, String> {
    let snapshot = read_bound_session_snapshot(account)?;
    read_real_usage_from_live_session(account, &snapshot).map_err(|error| error.message)
}

enum RealAccountSamplingOutcome {
    Updated(RealUsageReading),
    Unavailable,
    Mismatch,
    Expired,
}

fn collect_real_account_sampling_outcome(
    account: &Account,
) -> Result<RealAccountSamplingOutcome, String> {
    let verified = match verify_real_codex_session() {
        Ok(verified) => verified,
        Err(_) => return collect_bound_snapshot_sampling_outcome(account),
    };
    let live_snapshot = live_session_snapshot(&verified)?;
    let same_session = match account_matches_live_session(account, &verified, &live_snapshot) {
        Ok(value) => value,
        Err(error) => {
            if account_matches_verified_identity(account, &verified) {
                return Err(error);
            }
            return Ok(RealAccountSamplingOutcome::Expired);
        }
    };

    if same_session {
        return match read_real_usage_from_active_live_session(account, &live_snapshot) {
            Ok(Some(reading)) => Ok(RealAccountSamplingOutcome::Updated(reading)),
            Ok(None) => Ok(RealAccountSamplingOutcome::Unavailable),
            Err(error) => {
                if error.kind == RealUsageReadErrorKind::AuthInvalid {
                    Ok(RealAccountSamplingOutcome::Expired)
                } else {
                    Err(error.message)
                }
            }
        };
    }

    let bound_snapshot = match read_bound_session_snapshot(account) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if account_matches_verified_identity(account, &verified) {
                return Err(error);
            }
            return Ok(RealAccountSamplingOutcome::Expired);
        }
    };

    if !bound_snapshot_matches_account(account, &bound_snapshot) {
        return Ok(RealAccountSamplingOutcome::Mismatch);
    }

    match read_real_usage_from_live_session(account, &bound_snapshot) {
        Ok(Some(reading)) => Ok(RealAccountSamplingOutcome::Updated(reading)),
        Ok(None) => Ok(RealAccountSamplingOutcome::Unavailable),
        Err(error) => {
            if error.kind == RealUsageReadErrorKind::AuthInvalid {
                Ok(RealAccountSamplingOutcome::Expired)
            } else {
                Err(error.message)
            }
        }
    }
}

fn collect_bound_snapshot_sampling_outcome(
    account: &Account,
) -> Result<RealAccountSamplingOutcome, String> {
    let bound_snapshot = read_bound_session_snapshot(account)?;

    if !bound_snapshot_matches_account(account, &bound_snapshot) {
        return Ok(RealAccountSamplingOutcome::Mismatch);
    }

    match read_real_usage_from_live_session(account, &bound_snapshot) {
        Ok(Some(reading)) => Ok(RealAccountSamplingOutcome::Updated(reading)),
        Ok(None) => Ok(RealAccountSamplingOutcome::Unavailable),
        Err(error) => {
            if error.kind == RealUsageReadErrorKind::AuthInvalid {
                Ok(RealAccountSamplingOutcome::Expired)
            } else {
                Err(error.message)
            }
        }
    }
}

fn reading_regresses_before_reset(
    latest: &UsageSnapshot,
    reading: &RealUsageReading,
    now: chrono::DateTime<Local>,
) -> bool {
    let _ = (latest, reading, now);
    false
}

fn apply_background_sampling_outcome(
    connection: &Connection,
    account: &Account,
    now: chrono::DateTime<Local>,
    outcome: RealAccountSamplingOutcome,
    created_notifications: &mut i32,
    notification_limit: i32,
) -> Result<(), String> {
    let now_text = now.format("%Y-%m-%d %H:%M:%S").to_string();

    match outcome {
        RealAccountSamplingOutcome::Updated(reading) => {
            if let Some(latest) = query_latest_real_usage_snapshot(connection, account.id)? {
                if reading_regresses_before_reset(&latest, &reading, now) {
                    connection
                        .execute(
                            "UPDATE accounts SET status = ?1, auth_state = 'valid', last_verified_at = ?2, last_check_time = ?2, estimated_reset_time = ?3, updated_at = ?2 WHERE id = ?4",
                            params![
                                latest.risk_level,
                                now_text,
                                latest.estimated_reset_5h_at,
                                account.id,
                            ],
                        )
                        .map_err(|error| error.to_string())?;
                    return Ok(());
                }
            }
            insert_real_usage_snapshot(connection, account.id, now, &reading)?;
        }
        RealAccountSamplingOutcome::Unavailable => {
            connection
                .execute(
                    "UPDATE accounts SET status = 'healthy', auth_state = 'valid', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1 WHERE id = ?2",
                    params![now_text, account.id],
                )
                .map_err(|error| error.to_string())?;
            if *created_notifications < notification_limit {
                insert_account_notification(
                    connection,
                    account.id,
                    "info",
                    &format!("{} 真实额度暂不可读", account.nickname),
                    "当前已完成真实登录态校验，但还没有稳定真实额度读取链路；本轮保持 unknown 展示，不会写入任何非真实数据。",
                    "real_verification",
                    "sample_unavailable",
                    None,
                )?;
                *created_notifications += 1;
            }
        }
        RealAccountSamplingOutcome::Mismatch => {
            connection
                .execute(
                    "UPDATE accounts SET status = 'warning', auth_state = 'mismatch', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1 WHERE id = ?2",
                    params![now_text, account.id],
                )
                .map_err(|error| error.to_string())?;
            if *created_notifications < notification_limit {
                insert_account_notification(
                    connection,
                    account.id,
                    "warning",
                    &format!("{} 登录态异常", account.nickname),
                    "当前真实账号与官方登录态不一致；本轮仅更新校验状态，未生成真实用量快照。",
                    "real_verification",
                    "sample_mismatch",
                    None,
                )?;
                *created_notifications += 1;
            }
        }
        RealAccountSamplingOutcome::Expired => {
            if account.is_active {
                mark_account_auth_invalid(connection, account.id, &now_text)?;
            } else {
                connection
                    .execute(
                        "UPDATE accounts SET last_check_time = ?1, updated_at = ?1 WHERE id = ?2",
                        params![now_text, account.id],
                    )
                    .map_err(|error| error.to_string())?;
                if *created_notifications < notification_limit {
                    insert_account_notification(
                        connection,
                        account.id,
                        "warning",
                        &format!("{} 绑定快照暂不可读", account.nickname),
                        "非当前官方账号的绑定快照暂时无法读取真实额度；已保留上一次可信状态，切换或重新登录该账号时会再次校验。",
                        "real_verification",
                        "sample_inactive_auth_unconfirmed",
                        None,
                    )?;
                    *created_notifications += 1;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
fn sample_real_account_usage(
    connection: &Connection,
    account: &Account,
    now: chrono::DateTime<Local>,
) -> Result<bool, String> {
    let verified = verify_real_codex_session()?;
    let live_snapshot = live_session_snapshot(&verified)?;
    let now_text = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let same_session = match account_matches_live_session(account, &verified, &live_snapshot) {
        Ok(value) => value,
        Err(error) => {
            if account_matches_verified_identity(account, &verified) {
                return Err(error);
            }

            connection
                .execute(
                    "UPDATE accounts
                     SET status = 'auth_invalid', auth_state = 'expired', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1
                     WHERE id = ?2",
                    params![now_text, account.id],
                )
                .map_err(|db_error| db_error.to_string())?;
            return Ok(false);
        }
    };

    if same_session {
        match read_real_usage_from_active_live_session(account, &live_snapshot) {
            Ok(Some(reading)) => {
                insert_real_usage_snapshot(connection, account.id, now, &reading)?;
                return Ok(true);
            }
            Ok(None) => {}
            Err(error) => {
                return apply_real_usage_read_error(connection, account, &now_text, error)
            }
        }

        connection
            .execute(
                "UPDATE accounts SET status = 'healthy', auth_state = 'valid', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1 WHERE id = ?2",
                params![now_text, account.id],
            )
            .map_err(|error| error.to_string())?;
        return Ok(false);
    }

    let bound_snapshot = match read_bound_session_snapshot(account) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if account_matches_verified_identity(account, &verified) {
                return Err(error);
            }

            connection
                .execute(
                    "UPDATE accounts
                     SET status = 'auth_invalid', auth_state = 'expired', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1
                     WHERE id = ?2",
                    params![now_text, account.id],
                )
                .map_err(|db_error| db_error.to_string())?;
            return Ok(false);
        }
    };

    if !bound_snapshot_matches_account(account, &bound_snapshot) {
        connection
            .execute(
                "UPDATE accounts SET status = 'warning', auth_state = 'mismatch', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1 WHERE id = ?2",
                params![now_text, account.id],
            )
            .map_err(|error| error.to_string())?;
        return Ok(false);
    }

    match read_real_usage_from_live_session(account, &bound_snapshot) {
        Ok(Some(reading)) => {
            insert_real_usage_snapshot(connection, account.id, now, &reading)?;
            return Ok(true);
        }
        Ok(None) => {}
        Err(error) => return apply_real_usage_read_error(connection, account, &now_text, error),
    }

    connection
        .execute(
            "UPDATE accounts SET status = 'healthy', auth_state = 'valid', last_verified_at = ?1, last_check_time = ?1, estimated_reset_time = NULL, updated_at = ?1 WHERE id = ?2",
            params![now_text, account.id],
        )
        .map_err(|error| error.to_string())?;

    Ok(false)
}

#[cfg(test)]
fn run_resilient_sampling_cycle<F>(accounts: &[Account], mut sampler: F) -> Vec<String>
where
    F: FnMut(&Account) -> Result<bool, String>,
{
    let mut failures = Vec::new();

    for account in accounts {
        if let Err(error) = sampler(account) {
            failures.push(format!("{}：{}", account.nickname, error));
        }
    }

    failures
}

fn summarize_sampling_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return String::new();
    }

    format!("{} 个账号采样失败：{}", failures.len(), failures.join("；"))
}

fn mask_secret(value: &str) -> String {
    let trimmed = value.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "*".repeat(chars.len().max(4));
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{}...{}", prefix, suffix)
}

fn validate_api_key_secret(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("请输入 API key".to_string());
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") || lowered.contains("://")
    {
        return Err("API Key 不能填写 Base URL，请填写供应商后台生成的真实 key。".to_string());
    }
    Ok(())
}

fn validate_usage_access_secret(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("请输入访问令牌".to_string());
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") || lowered.contains("://")
    {
        return Err("访问令牌不能填写 URL，请填写 oneTop 安全设置里的访问令牌。".to_string());
    }
    Ok(())
}

fn usage_secret_key_for_profile(profile_id: i64) -> String {
    format!("key-usage-profile-{profile_id}")
}

fn query_credential_profile_by_id(
    connection: &Connection,
    id: i64,
) -> Result<CredentialProfile, String> {
    connection
        .query_row(
            "SELECT id, profile_kind, provider, nickname, status, is_active,
                    base_url, model, masked_secret, secret_ref, linked_account_id,
                    usage_provider_type, usage_query_user, usage_query_app_version,
                    usage_masked_secret, usage_secret_ref
             FROM credential_profiles
             WHERE id = ?1",
            [id],
            |row| {
                Ok(CredentialProfile {
                    id: row.get(0)?,
                    profile_kind: row.get(1)?,
                    provider: row.get(2)?,
                    nickname: row.get(3)?,
                    status: row.get(4)?,
                    is_active: row.get::<_, i64>(5)? == 1,
                    base_url: row.get(6)?,
                    model: row.get(7)?,
                    masked_secret: row.get(8)?,
                    secret_ref: row.get(9)?,
                    linked_account_id: row.get(10)?,
                    usage_provider_type: row.get(11)?,
                    usage_query_user: row.get(12)?,
                    usage_query_app_version: row.get(13)?,
                    usage_masked_secret: row.get(14)?,
                    usage_secret_ref: row.get(15)?,
                    usage_summary: None,
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn create_key_profile_record(
    connection: &Connection,
    input: CreateKeyProfileInput,
    secret_ref: &str,
) -> Result<CredentialProfile, String> {
    let provider = input.provider.trim();
    let nickname = input.nickname.trim();
    let base_url = input.base_url.trim();
    let model = input.model.trim();
    let api_key = input.api_key.trim();

    if provider.is_empty() {
        return Err("请输入供应商名称".to_string());
    }
    if nickname.is_empty() {
        return Err("请输入 key 昵称".to_string());
    }
    if base_url.is_empty() {
        return Err("请输入 base URL".to_string());
    }
    if model.is_empty() {
        return Err("请输入模型名称".to_string());
    }
    if api_key.is_empty() {
        return Err("请输入 API key".to_string());
    }
    validate_api_key_secret(api_key)?;

    let now = now_text();
    connection
        .execute(
            "INSERT INTO credential_profiles (
                profile_kind, provider, nickname, status, is_active, base_url, model,
                masked_secret, secret_ref, created_at, updated_at
             )
             VALUES ('third_party_key', ?1, ?2, 'unknown', 0, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                provider,
                nickname,
                base_url,
                model,
                mask_secret(api_key),
                secret_ref.trim(),
                now,
            ],
        )
        .map_err(|error| error.to_string())?;

    query_credential_profile_by_id(connection, connection.last_insert_rowid())
}

fn update_key_profile_record(
    connection: &Connection,
    input: &UpdateKeyProfileInput,
    masked_secret: Option<&str>,
) -> Result<CredentialProfile, String> {
    let existing = query_credential_profile_by_id(connection, input.id)?;
    if existing.profile_kind != "third_party_key" {
        return Err("只有第三方 key 支持编辑。".to_string());
    }

    let provider = input.provider.trim();
    let nickname = input.nickname.trim();
    let base_url = input.base_url.trim();
    let model = input.model.trim();

    if provider.is_empty() {
        return Err("请输入供应商名称".to_string());
    }
    if nickname.is_empty() {
        return Err("请输入 key 昵称".to_string());
    }
    if base_url.is_empty() {
        return Err("请输入 base URL".to_string());
    }
    if model.is_empty() {
        return Err("请输入模型名称".to_string());
    }

    let now = now_text();
    if let Some(masked_secret) = masked_secret {
        connection
            .execute(
                "UPDATE credential_profiles
                 SET provider = ?1,
                     nickname = ?2,
                     base_url = ?3,
                     model = ?4,
                     masked_secret = ?5,
                     status = 'unknown',
                     updated_at = ?6
                 WHERE id = ?7",
                params![
                    provider,
                    nickname,
                    base_url,
                    model,
                    masked_secret,
                    now,
                    input.id
                ],
            )
            .map_err(|error| error.to_string())?;
    } else {
        connection
            .execute(
                "UPDATE credential_profiles
                 SET provider = ?1,
                     nickname = ?2,
                     base_url = ?3,
                     model = ?4,
                     status = 'unknown',
                     updated_at = ?5
                 WHERE id = ?6",
                params![provider, nickname, base_url, model, now, input.id],
            )
            .map_err(|error| error.to_string())?;
    }

    query_credential_profile_by_id(connection, input.id)
}

fn update_key_profile_usage_config_record(
    connection: &Connection,
    input: &UpdateKeyProfileUsageConfigInput,
    usage_masked_secret: Option<Option<String>>,
    usage_secret_ref: Option<Option<String>>,
) -> Result<CredentialProfile, String> {
    let existing = query_credential_profile_by_id(connection, input.profile_id)?;
    if existing.profile_kind != "third_party_key" {
        return Err("只有第三方 key 支持编辑余额统计配置。".to_string());
    }

    let normalized_type = normalized_usage_provider_type(input.usage_provider_type.as_deref());
    let normalized_user = normalize_optional_text(input.usage_query_user.as_deref());
    let normalized_app_version = normalize_optional_text(input.usage_query_app_version.as_deref());
    let now = now_text();

    let final_masked_secret = usage_masked_secret.unwrap_or(existing.usage_masked_secret.clone());
    let final_secret_ref = usage_secret_ref.unwrap_or(existing.usage_secret_ref.clone());

    let (
        usage_provider_type,
        usage_query_user,
        usage_query_app_version,
        usage_masked_secret,
        usage_secret_ref,
    ) = if normalized_type.is_none() {
        (None, None, None, None, None)
    } else {
        (
            normalized_type,
            normalized_user,
            normalized_app_version,
            final_masked_secret,
            final_secret_ref,
        )
    };

    connection
        .execute(
            "UPDATE credential_profiles
             SET usage_provider_type = ?1,
                 usage_query_user = ?2,
                 usage_query_app_version = ?3,
                 usage_masked_secret = ?4,
                 usage_secret_ref = ?5,
                 updated_at = ?6
             WHERE id = ?7",
            params![
                usage_provider_type,
                usage_query_user,
                usage_query_app_version,
                usage_masked_secret,
                usage_secret_ref,
                now,
                input.profile_id
            ],
        )
        .map_err(|error| error.to_string())?;

    query_credential_profile_by_id(connection, input.profile_id)
}

fn delete_profile_secret_ref(secret_ref: Option<&str>) -> Result<(), String> {
    if let Some(account_key) = secret_ref.and_then(keychain_account_key) {
        delete_account_secret(account_key)?;
    }
    Ok(())
}

fn delete_credential_profile_record(
    connection: &Connection,
    profile_id: i64,
) -> Result<(), String> {
    let profile = query_credential_profile_by_id(connection, profile_id)?;
    if profile.profile_kind != "third_party_key" {
        return Err("官方账号资产不能删除。".to_string());
    }
    if profile.is_active {
        return Err("当前登录的 Key 不能删除，请先切换到其他身份。".to_string());
    }

    delete_profile_secret_ref(profile.secret_ref.as_deref())?;
    delete_profile_secret_ref(profile.usage_secret_ref.as_deref())?;

    connection
        .execute(
            "DELETE FROM credential_profiles
             WHERE id = ?1 AND profile_kind = 'third_party_key'",
            [profile_id],
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn query_credential_profiles(connection: &Connection) -> Result<Vec<CredentialProfile>, String> {
    sync_account_credential_profiles(connection)?;

    let mut stmt = connection
        .prepare(
            "SELECT id, profile_kind, provider, nickname, status, is_active,
                    base_url, model, masked_secret, secret_ref, linked_account_id,
                    usage_provider_type, usage_query_user, usage_query_app_version,
                    usage_masked_secret, usage_secret_ref
             FROM credential_profiles
             ORDER BY is_active DESC, profile_kind ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CredentialProfile {
                id: row.get(0)?,
                profile_kind: row.get(1)?,
                provider: row.get(2)?,
                nickname: row.get(3)?,
                status: row.get(4)?,
                is_active: row.get::<_, i64>(5)? == 1,
                base_url: row.get(6)?,
                model: row.get(7)?,
                masked_secret: row.get(8)?,
                secret_ref: row.get(9)?,
                linked_account_id: row.get(10)?,
                usage_provider_type: row.get(11)?,
                usage_query_user: row.get(12)?,
                usage_query_app_version: row.get(13)?,
                usage_masked_secret: row.get(14)?,
                usage_secret_ref: row.get(15)?,
                usage_summary: None,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn activate_credential_profile_record(
    connection: &Connection,
    profile_id: i64,
) -> Result<CredentialProfile, String> {
    let profile = query_credential_profile_by_id(connection, profile_id)?;
    let now = now_text();
    connection
        .execute("UPDATE credential_profiles SET is_active = 0", [])
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE credential_profiles SET is_active = 1, updated_at = ?1 WHERE id = ?2",
            params![now, profile_id],
        )
        .map_err(|error| error.to_string())?;

    match profile.profile_kind.as_str() {
        "third_party_key" => {
            connection
                .execute("UPDATE accounts SET is_active = 0", [])
                .map_err(|error| error.to_string())?;
        }
        "official_account" => {
            if let Some(account_id) = profile.linked_account_id {
                set_active_account(connection, account_id)?;
            }
        }
        _ => {}
    }

    query_credential_profile_by_id(connection, profile_id)
}

fn activate_account_credential_profile_record(
    connection: &Connection,
    account_id: i64,
) -> Result<CredentialProfile, String> {
    sync_account_credential_profiles(connection)?;
    let profile_id = connection
        .query_row(
            "SELECT id FROM credential_profiles
             WHERE profile_kind = 'official_account' AND linked_account_id = ?1
             LIMIT 1",
            [account_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| "未找到该官方账号对应的账号资产。".to_string())?;
    activate_credential_profile_record(connection, profile_id)
}

fn escape_json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn escape_toml_basic_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn parse_toml_inline_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = value.strip_prefix('"') {
        let mut escaped = false;
        let mut parsed = String::new();
        for character in rest.chars() {
            if escaped {
                match character {
                    '"' => parsed.push('"'),
                    '\\' => parsed.push('\\'),
                    'n' => parsed.push('\n'),
                    'r' => parsed.push('\r'),
                    't' => parsed.push('\t'),
                    other => parsed.push(other),
                }
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '"' => return Some(parsed.trim().to_string()).filter(|item| !item.is_empty()),
                other => parsed.push(other),
            }
        }
        return None;
    }

    if let Some(rest) = value.strip_prefix('\'') {
        let parsed = rest.split('\'').next()?.trim().to_string();
        return Some(parsed).filter(|item| !item.is_empty());
    }

    let parsed = value
        .split('#')
        .next()
        .unwrap_or(value)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    Some(parsed).filter(|item| !item.is_empty())
}

fn read_codex_config_model_provider_from_dir(config_dir: &Path) -> Option<String> {
    let content = fs::read_to_string(config_dir.join("config.toml")).ok()?;
    let mut in_root_table = true;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_root_table = false;
            continue;
        }
        if !in_root_table {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "model_provider" {
            continue;
        }
        return parse_toml_inline_string(value);
    }
    None
}

fn key_profile_model_provider(profile: &CredentialProfile) -> String {
    // `model_provider` is persisted in Codex's shared thread index.  A human
    // provider name (for example "taomu") is not unique when more than one
    // Key uses the same gateway, so it cannot be used as a session owner.
    format!("codexswitcher-key-{}", profile.id)
}

fn key_profile_display_name(profile: &CredentialProfile) -> String {
    let provider = profile.provider.trim();
    if !provider.is_empty() {
        return provider.to_string();
    }

    let nickname = profile.nickname.trim();
    if !nickname.is_empty() {
        return nickname.to_string();
    }

    "第三方 Key".to_string()
}

fn normalize_openai_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") || trimmed.contains("/v1/") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalized_usage_provider_type(value: Option<&str>) -> Option<String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("sub2api") => Some("sub2api".to_string()),
        Some("new_api") | Some("newApi") => Some("new_api".to_string()),
        Some("none") => None,
        _ => None,
    }
}

fn third_party_usage_endpoint_from_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("第三方 key 缺少 base URL，无法查询余额。".to_string());
    }
    Ok(format!(
        "{}/usage",
        normalize_openai_base_url(trimmed).trim_end_matches('/')
    ))
}

fn new_api_usage_endpoint_from_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("第三方 key 缺少 base URL，无法查询余额。".to_string());
    }
    Ok(format!("{trimmed}/api/user/self"))
}

fn key_profile_usage_secret(profile: &CredentialProfile) -> Result<String, String> {
    let secret_ref = profile
        .usage_secret_ref
        .as_deref()
        .ok_or_else(|| "第三方 key 缺少余额统计访问令牌。".to_string())?;
    let account_key = keychain_account_key(secret_ref)
        .ok_or_else(|| "第三方 key 的余额统计令牌引用格式无效。".to_string())?;
    load_account_secret(account_key)
}

fn build_usage_detail_item(
    label: impl Into<String>,
    value: impl Into<String>,
) -> ThirdPartyKeyUsageDetailItem {
    ThirdPartyKeyUsageDetailItem {
        label: label.into(),
        value: value.into(),
    }
}

fn normalize_third_party_key_usage_bucket(
    bucket: ThirdPartyKeyUsageApiBucket,
) -> ThirdPartyKeyUsageBucket {
    ThirdPartyKeyUsageBucket {
        requests: bucket.requests.unwrap_or(0),
        input_tokens: bucket.input_tokens.unwrap_or(0),
        output_tokens: bucket.output_tokens.unwrap_or(0),
        cache_creation_tokens: bucket.cache_creation_tokens.unwrap_or(0),
        cache_read_tokens: bucket.cache_read_tokens.unwrap_or(0),
        total_tokens: bucket.total_tokens.unwrap_or(0),
        cost: bucket.cost.unwrap_or(0.0),
        actual_cost: bucket.actual_cost.unwrap_or(0.0),
        account_cost: bucket.account_cost,
    }
}

fn normalize_third_party_key_usage_model_stat(
    stat: ThirdPartyKeyUsageApiModelStat,
) -> ThirdPartyKeyUsageModelStat {
    ThirdPartyKeyUsageModelStat {
        model: stat.model.unwrap_or_else(|| "unknown".to_string()),
        requests: stat.requests.unwrap_or(0),
        input_tokens: stat.input_tokens.unwrap_or(0),
        output_tokens: stat.output_tokens.unwrap_or(0),
        cache_creation_tokens: stat.cache_creation_tokens.unwrap_or(0),
        cache_read_tokens: stat.cache_read_tokens.unwrap_or(0),
        total_tokens: stat.total_tokens.unwrap_or(0),
        cost: stat.cost.unwrap_or(0.0),
        actual_cost: stat.actual_cost.unwrap_or(0.0),
        account_cost: stat.account_cost,
    }
}

fn build_third_party_key_usage_summary(
    usage_endpoint: String,
    payload: ThirdPartyKeyUsageApiResponse,
) -> ThirdPartyKeyUsageSummary {
    let today = payload
        .usage
        .today
        .map(normalize_third_party_key_usage_bucket);
    let total = payload
        .usage
        .total
        .map(normalize_third_party_key_usage_bucket);
    let detail_items = [
        today
            .as_ref()
            .map(|item| build_usage_detail_item("今日请求", format!("{} 次", item.requests))),
        today
            .as_ref()
            .map(|item| build_usage_detail_item("今日费用", format!("{:.2}", item.cost))),
        total
            .as_ref()
            .map(|item| build_usage_detail_item("累计请求", format!("{} 次", item.requests))),
        total
            .as_ref()
            .map(|item| build_usage_detail_item("累计费用", format!("{:.2}", item.cost))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    ThirdPartyKeyUsageSummary {
        status: "ready".to_string(),
        message: None,
        fetched_at: now_text(),
        usage_endpoint: Some(usage_endpoint),
        usage_provider_type: Some("sub2api".to_string()),
        balance: payload.balance,
        remaining: payload.remaining,
        unit: payload.unit,
        is_valid: payload.is_valid,
        mode: payload.mode,
        plan_name: payload.plan_name,
        average_duration_ms: payload.usage.average_duration_ms,
        rpm: payload.usage.rpm,
        tpm: payload.usage.tpm,
        today,
        total,
        model_stats: payload
            .model_stats
            .into_iter()
            .map(normalize_third_party_key_usage_model_stat)
            .collect(),
        detail_items,
    }
}

fn build_new_api_usage_summary(
    usage_endpoint: String,
    query_user: &str,
    payload: NewApiUserSelfResponse,
) -> Option<ThirdPartyKeyUsageSummary> {
    let data = payload.data?;
    let group = normalize_optional_text(data.group.as_deref()).filter(|value| value != "default");
    let account_name = normalize_optional_text(data.display_name.as_deref())
        .or_else(|| normalize_optional_text(data.username.as_deref()));
    let quota = data.quota.map(|value| value / 500000.0);
    let used_quota = data.used_quota.map(|value| value / 500000.0);
    let total_quota = match (quota, used_quota) {
        (Some(remaining), Some(used)) => Some(remaining + used),
        _ => None,
    };
    let request_count = data.request_count;

    let mut detail_items = Vec::new();
    if let Some(value) = request_count {
        detail_items.push(build_usage_detail_item("累计请求", format!("{value} 次")));
    }
    if let Some(value) = used_quota {
        detail_items.push(build_usage_detail_item(
            "已用金额",
            format!("USD {:.2}", value),
        ));
    }
    if let Some(value) = total_quota {
        detail_items.push(build_usage_detail_item(
            "总额度",
            format!("USD {:.2}", value),
        ));
    }
    if let Some(value) = data.id {
        detail_items.push(build_usage_detail_item("用户 ID", value.to_string()));
    } else {
        detail_items.push(build_usage_detail_item("用户 ID", query_user.to_string()));
    }
    if let Some(value) = account_name.clone() {
        detail_items.push(build_usage_detail_item("账号", value));
    }

    Some(ThirdPartyKeyUsageSummary {
        status: "ready".to_string(),
        message: None,
        fetched_at: now_text(),
        usage_endpoint: Some(usage_endpoint),
        usage_provider_type: Some("new_api".to_string()),
        balance: quota,
        remaining: quota,
        unit: Some("USD".to_string()),
        is_valid: payload
            .success
            .or(Some(true))
            .map(|success| success && data.status.unwrap_or(1) == 1),
        mode: None,
        plan_name: group,
        average_duration_ms: None,
        rpm: None,
        tpm: None,
        today: None,
        total: Some(ThirdPartyKeyUsageBucket {
            requests: request_count.unwrap_or(0),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            total_tokens: total_quota.unwrap_or(0.0).round() as i64,
            cost: 0.0,
            actual_cost: 0.0,
            account_cost: None,
        }),
        model_stats: Vec::new(),
        detail_items,
    })
}

fn shorten_remote_response_for_error(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

fn fetch_sub2api_usage_summary(profile: &CredentialProfile) -> Option<ThirdPartyKeyUsageSummary> {
    let usage_endpoint = profile
        .base_url
        .as_deref()
        .map(third_party_usage_endpoint_from_base_url)
        .transpose()
        .ok()??;
    let api_key = key_profile_secret(profile).ok()?;
    let client = Client::builder()
        .timeout(StdDuration::from_secs(6))
        .build()
        .ok()?;
    let response = client
        .get(&usage_endpoint)
        .bearer_auth(api_key.trim())
        .header("Accept", "application/json")
        .send()
        .ok()?;
    if !response.status().is_success() {
        let _ = shorten_remote_response_for_error(&response.text().unwrap_or_default());
        return None;
    }
    let payload = response.json::<ThirdPartyKeyUsageApiResponse>().ok()?;
    Some(build_third_party_key_usage_summary(usage_endpoint, payload))
}

fn fetch_new_api_usage_summary(profile: &CredentialProfile) -> Option<ThirdPartyKeyUsageSummary> {
    let usage_endpoint = profile
        .base_url
        .as_deref()
        .map(new_api_usage_endpoint_from_base_url)
        .transpose()
        .ok()??;
    let access_token = key_profile_usage_secret(profile).ok()?;
    let query_user = normalize_optional_text(profile.usage_query_user.as_deref())?;
    let app_version = normalize_optional_text(profile.usage_query_app_version.as_deref())
        .unwrap_or_else(|| "3.1.0".to_string());
    let client = Client::builder()
        .timeout(StdDuration::from_secs(6))
        .build()
        .ok()?;
    let response = client
        .get(&usage_endpoint)
        .bearer_auth(access_token.trim())
        .header("Accept", "application/json")
        .header("App-Version", app_version)
        .header("New-Api-User", query_user.as_str())
        .send()
        .ok()?;
    if !response.status().is_success() {
        let _ = shorten_remote_response_for_error(&response.text().unwrap_or_default());
        return None;
    }
    let payload = response.json::<NewApiUserSelfResponse>().ok()?;
    build_new_api_usage_summary(usage_endpoint, query_user.as_str(), payload)
}

fn fetch_third_party_key_usage_summary(
    profile: &CredentialProfile,
) -> Option<ThirdPartyKeyUsageSummary> {
    match normalized_usage_provider_type(profile.usage_provider_type.as_deref()).as_deref() {
        Some("sub2api") => fetch_sub2api_usage_summary(profile),
        Some("new_api") => fetch_new_api_usage_summary(profile),
        _ => None,
    }
}

fn official_account_config_toml() -> String {
    "model = \"gpt-5-codex\"\nmodel_reasoning_effort = \"high\"\n".to_string()
}

fn write_official_account_runtime_files(
    config_dir: &Path,
    credentials_json: &str,
) -> Result<(), String> {
    let credentials_json = credentials_json.trim();
    if credentials_json.is_empty() {
        return Err("官方账号凭证为空，无法恢复官方运行配置。".to_string());
    }

    fs::create_dir_all(config_dir)
        .map_err(|error| format!("创建 Codex 配置目录失败：{}", error))?;
    fs::write(
        config_dir.join("auth.json"),
        format!("{credentials_json}\n"),
    )
    .map_err(|error| format!("写入 Codex auth.json 失败：{}", error))?;
    fs::write(
        config_dir.join("config.toml"),
        official_account_config_toml(),
    )
    .map_err(|error| format!("重置 Codex config.toml 失败：{}", error))?;
    Ok(())
}

fn apply_official_account_runtime_config(account: &Account) -> Result<(), String> {
    if !account.is_real_session || account.binding_kind != "codex_cli" {
        return Err("只有官方 Codex 账号可以恢复官方运行配置。".to_string());
    }

    let snapshot = read_bound_session_snapshot(account)?;
    let config_dir = codex_config_dir()?;
    write_official_account_runtime_files(&config_dir, &snapshot.credentials_json)
}

fn key_profile_secret(profile: &CredentialProfile) -> Result<String, String> {
    let secret_ref = profile
        .secret_ref
        .as_deref()
        .ok_or_else(|| "第三方 key 缺少 Keychain 引用。".to_string())?;
    let account_key = keychain_account_key(secret_ref)
        .ok_or_else(|| "第三方 key 的 Keychain 引用格式无效。".to_string())?;
    load_account_secret(account_key)
}

fn write_key_profile_runtime_files(
    config_dir: &Path,
    profile: &CredentialProfile,
    api_key: &str,
) -> Result<(), String> {
    if profile.profile_kind != "third_party_key" {
        return Err("只有第三方 key 身份可以写入 key 运行配置。".to_string());
    }

    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "第三方 key 缺少 base URL，无法启用。".to_string())?;
    let base_url = normalize_openai_base_url(base_url);
    let model = profile
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "第三方 key 缺少模型名称，无法启用。".to_string())?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("第三方 key 为空，无法启用。".to_string());
    }

    fs::create_dir_all(config_dir)
        .map_err(|error| format!("创建 Codex 配置目录失败：{}", error))?;

    let model_provider = key_profile_model_provider(profile);
    let display_name = key_profile_display_name(profile);
    let auth_json = format!(
        "{{\"OPENAI_API_KEY\":\"{}\"}}\n",
        escape_json_string(api_key)
    );
    let config_toml = format!(
        "model_provider = \"{}\"\nmodel = \"{}\"\ndisable_response_storage = true\n\n[model_providers.\"{}\"]\nname = \"{}\"\nbase_url = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
        escape_toml_basic_string(&model_provider),
        escape_toml_basic_string(model),
        escape_toml_basic_string(&model_provider),
        escape_toml_basic_string(&display_name),
        escape_toml_basic_string(&base_url),
    );

    fs::write(config_dir.join("auth.json"), auth_json)
        .map_err(|error| format!("写入 Codex auth.json 失败：{}", error))?;
    fs::write(config_dir.join("config.toml"), config_toml)
        .map_err(|error| format!("写入 Codex config.toml 失败：{}", error))?;

    Ok(())
}

fn apply_key_profile_runtime_config(profile: &CredentialProfile) -> Result<(), String> {
    let api_key = key_profile_secret(profile)?;
    let config_dir = codex_config_dir()?;
    write_key_profile_runtime_files(&config_dir, profile, &api_key)
}

fn runtime_openai_api_key(status: &CodexAuthStatus) -> Option<String> {
    let auth_value = serde_json::from_str::<Value>(&status.session_json).ok()?;
    auth_value
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn active_account_id_for_runtime_login(
    connection: &Connection,
    status: &CodexAuthStatus,
) -> Result<Option<i64>, String> {
    if !status.logged_in {
        return Ok(None);
    }

    Ok(query_accounts(connection)?.into_iter().find_map(|account| {
        let email_matches = status
            .account_email
            .as_deref()
            .is_some_and(|email| account.account_email.as_deref() == Some(email));
        let id_matches = status
            .account_id
            .as_deref()
            .is_some_and(|account_id| account.profile_ref.as_deref() == Some(account_id));
        (email_matches || id_matches).then_some(account.id)
    }))
}

fn key_profile_id_for_runtime_secret(
    connection: &Connection,
    runtime_key: &str,
) -> Result<Option<i64>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, secret_ref FROM credential_profiles
             WHERE profile_kind = 'third_party_key' AND secret_ref IS NOT NULL
             ORDER BY id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;

    for row in rows {
        let (profile_id, secret_ref) = row.map_err(|error| error.to_string())?;
        let Some(account_key) = keychain_account_key(&secret_ref) else {
            continue;
        };
        if load_account_secret(account_key)
            .map(|secret| secret.trim() == runtime_key.trim())
            .unwrap_or(false)
        {
            return Ok(Some(profile_id));
        }
    }

    Ok(None)
}

fn reconcile_runtime_active_identity(connection: &Connection) -> Result<(), String> {
    let status = match read_codex_auth_status_cached() {
        Ok(status) => status,
        Err(_) => return Ok(()),
    };

    if let Some(account_id) = active_account_id_for_runtime_login(connection, &status)? {
        set_active_account(connection, account_id)?;
        connection
            .execute("UPDATE credential_profiles SET is_active = 0", [])
            .map_err(|error| error.to_string())?;
        sync_account_credential_profiles(connection)?;
        return Ok(());
    }

    if let Some(runtime_key) = runtime_openai_api_key(&status) {
        if let Some(profile_id) = key_profile_id_for_runtime_secret(connection, &runtime_key)? {
            activate_credential_profile_record(connection, profile_id)?;
        } else {
            connection
                .execute("UPDATE accounts SET is_active = 0", [])
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn recover_inactive_accounts_with_trusted_snapshot(connection: &Connection) -> Result<(), String> {
    let mut stmt = connection
        .prepare(
            "SELECT id FROM accounts
             WHERE is_real_session = 1
               AND binding_kind = 'codex_cli'
               AND is_active = 0
               AND status = 'auth_invalid'
               AND auth_state = 'expired'",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?;
    let account_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(stmt);

    for account_id in account_ids {
        let account = query_account_by_id(connection, account_id)?;
        let Some(snapshot) = account.latest_snapshot.as_ref() else {
            continue;
        };
        if snapshot.source_type != "real_usage" {
            continue;
        }

        let Ok(bound_snapshot) = read_bound_session_snapshot(&account) else {
            continue;
        };
        if !bound_snapshot_matches_account(&account, &bound_snapshot) {
            continue;
        }

        connection
            .execute(
                "UPDATE accounts
                 SET status = ?1,
                     auth_state = 'valid',
                     updated_at = ?2
                 WHERE id = ?3",
                params![snapshot.risk_level, now_text(), account_id],
            )
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn exhausted_snapshot_reset_due_at(snapshot: &UsageSnapshot, now: chrono::DateTime<Local>) -> bool {
    if snapshot.risk_level != "exhausted" {
        return false;
    }

    let exhausted_windows = [
        (
            snapshot.window_5h_percent,
            snapshot.estimated_reset_5h_at.as_deref(),
        ),
        (
            snapshot.window_7d_percent,
            snapshot.estimated_reset_7d_at.as_deref(),
        ),
    ]
    .into_iter()
    .filter(|(percent, _)| *percent >= 100)
    .collect::<Vec<_>>();

    !exhausted_windows.is_empty()
        && exhausted_windows.into_iter().all(|(_, reset_at)| {
            reset_at
                .and_then(parse_local_datetime_text)
                .is_some_and(|reset_at| reset_at <= now)
        })
}

fn effective_account_status_from_snapshot(snapshot: &UsageSnapshot) -> String {
    if exhausted_snapshot_reset_due_at(snapshot, Local::now()) {
        "warning".to_string()
    } else {
        snapshot.risk_level.clone()
    }
}

fn sync_account_credential_profiles(connection: &Connection) -> Result<(), String> {
    let has_active_key = connection
        .query_row(
            "SELECT COUNT(*) FROM credential_profiles
             WHERE profile_kind = 'third_party_key' AND is_active = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    let mut stmt = connection
        .prepare(
            "SELECT id, provider, nickname, status, is_active
             FROM accounts
             WHERE is_real_session = 1 AND binding_kind = 'codex_cli'
             ORDER BY id ASC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let accounts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    for (account_id, provider, nickname, status, is_active) in accounts {
        let now = now_text();
        let profile_is_active = if has_active_key { 0 } else { is_active };
        let existing_id = connection
            .query_row(
                "SELECT id FROM credential_profiles
                 WHERE profile_kind = 'official_account' AND linked_account_id = ?1
                 LIMIT 1",
                [account_id],
                |row| row.get::<_, i64>(0),
            )
            .ok();

        if let Some(profile_id) = existing_id {
            connection
                .execute(
                    "UPDATE credential_profiles
                     SET provider = ?1, nickname = ?2, status = ?3, is_active = ?4, updated_at = ?5
                     WHERE id = ?6",
                    params![
                        provider,
                        nickname,
                        status,
                        profile_is_active,
                        now,
                        profile_id
                    ],
                )
                .map_err(|error| error.to_string())?;
        } else {
            connection
                .execute(
                    "INSERT INTO credential_profiles (
                        profile_kind, provider, nickname, status, is_active,
                        linked_account_id, created_at, updated_at
                     )
                     VALUES ('official_account', ?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![
                        provider,
                        nickname,
                        status,
                        profile_is_active,
                        account_id,
                        now
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn query_accounts(connection: &Connection) -> Result<Vec<Account>, String> {
    recover_inactive_accounts_with_trusted_snapshot(connection)?;

    let mut stmt = connection
        .prepare(
            "SELECT id, provider, nickname, status, is_active, is_default, auth_state, last_check_time, estimated_reset_time,
                    account_key, binding_kind, session_ref, profile_ref, account_email, last_verified_at, is_real_session
             FROM accounts
             ORDER BY is_default DESC, id ASC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                provider: row.get(1)?,
                nickname: row.get(2)?,
                status: row.get(3)?,
                is_active: row.get::<_, i64>(4)? == 1,
                is_default: row.get::<_, i64>(5)? == 1,
                auth_state: row.get(6)?,
                last_check_time: row.get(7)?,
                estimated_reset_time: row.get(8)?,
                account_key: row.get(9)?,
                binding_kind: row.get(10)?,
                session_ref: row.get(11)?,
                profile_ref: row.get(12)?,
                account_email: row.get(13)?,
                last_verified_at: row.get(14)?,
                is_real_session: row.get::<_, i64>(15)? == 1,
                plan_label: None,
                latest_snapshot: None,
            })
        })
        .map_err(|error| error.to_string())?;

    let mut accounts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    for account in &mut accounts {
        account.latest_snapshot = query_latest_real_usage_snapshot(connection, account.id)?;
        account.plan_label = query_latest_account_plan_label(connection, account.id)?;
        if let Some(snapshot) = &account.latest_snapshot {
            if account.auth_state == "valid" && snapshot.source_type == "real_usage" {
                account.status = effective_account_status_from_snapshot(snapshot);
            }
        }
    }

    Ok(accounts)
}

fn query_settings(connection: &Connection) -> Result<AppSettings, String> {
    connection
        .query_row(
            "SELECT warn_threshold_low, warn_threshold_mid, warn_threshold_high, check_interval, enable_handoff, prefer_official_upgrade, enable_auto_refresh, enable_auto_sampling, foreground_auto_sampling_only, launch_at_login, menu_bar_only
             FROM app_settings WHERE id = 1",
            [],
            |row| {
                Ok(AppSettings {
                    warn_threshold_low: row.get(0)?,
                    warn_threshold_mid: row.get(1)?,
                    warn_threshold_high: row.get(2)?,
                    check_interval: row.get(3)?,
                    enable_handoff: row.get::<_, i64>(4)? == 1,
                    prefer_official_upgrade: row.get::<_, i64>(5)? == 1,
                    enable_auto_refresh: row.get::<_, i64>(6)? == 1,
                    enable_auto_sampling: row.get::<_, i64>(7)? == 1,
                    foreground_auto_sampling_only: row.get::<_, i64>(8)? == 1,
                    launch_at_login: row.get::<_, i64>(9)? == 1,
                    menu_bar_only: row.get::<_, i64>(10)? == 1,
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn query_local_projects(connection: &Connection) -> Result<Vec<LocalProject>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, name, workspace_path, git_remote, last_active_at, created_at, updated_at
             FROM local_projects
             ORDER BY COALESCE(last_active_at, updated_at) DESC, id DESC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(LocalProject {
                id: row.get(0)?,
                name: row.get(1)?,
                workspace_path: row.get(2)?,
                git_remote: row.get(3)?,
                last_active_at: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn map_session_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        project_name: row.get(2)?,
        project_path: row.get(3)?,
        owner_account_id: row.get(4)?,
        owner_profile_kind: row.get(5)?,
        owner_profile_ref: row.get(6)?,
        record_type: row.get(7)?,
        title: row.get(8)?,
        summary: row.get(9)?,
        raw_content: row.get(10)?,
        message_count: row.get(11)?,
        source_record_id: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn query_session_records(connection: &Connection) -> Result<Vec<SessionRecord>, String> {
    backfill_codex_imported_session_titles(connection)?;
    backfill_codex_imported_session_owners(connection)?;
    backfill_codex_imported_state_model_providers(connection)?;
    let mut stmt = connection
        .prepare(
            "SELECT s.id, s.project_id, p.name, p.workspace_path, s.owner_account_id,
                    s.owner_profile_kind, s.owner_profile_ref, s.record_type, s.title, s.summary,
                    s.raw_content, s.message_count, s.source_record_id, s.created_at, s.updated_at
             FROM session_records s
             JOIN local_projects p ON p.id = s.project_id
             ORDER BY s.updated_at DESC, s.id DESC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], map_session_record)
        .map_err(|error| error.to_string())?;

    let non_main_thread_ids = if cfg!(test) {
        None
    } else {
        codex_config_dir()
            .ok()
            .and_then(|codex_dir| read_codex_non_main_thread_ids(&codex_dir).ok())
            .flatten()
    };

    Ok(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|record| !is_internal_codex_review_title(&record.title))
        .filter(|record| is_main_codex_session_record(record, non_main_thread_ids.as_ref()))
        .collect())
}

fn query_notifications(connection: &Connection) -> Result<Vec<NotificationItem>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, account_id, level, title, message, source_type, action_type, related_handoff_id, created_at
             FROM notifications ORDER BY id DESC LIMIT 40",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(NotificationItem {
                id: row.get(0)?,
                account_id: row.get(1)?,
                level: row.get(2)?,
                title: row.get(3)?,
                message: row.get(4)?,
                source_type: row.get(5)?,
                action_type: row.get(6)?,
                related_handoff_id: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn query_workspace_support_data(connection: &Connection) -> Result<WorkspaceSupportData, String> {
    Ok(WorkspaceSupportData {
        projects: query_local_projects(connection)?,
        sessions: query_session_records(connection)?,
        notifications: query_notifications(connection)?,
    })
}

fn format_codex_timestamp(value: Option<&str>) -> String {
    value
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(now_text)
}

fn format_codex_unix_timestamp(value: i64) -> String {
    let timestamp = if value > 10_000_000_000 {
        Local.timestamp_millis_opt(value).single()
    } else {
        Local.timestamp_opt(value, 0).single()
    };
    timestamp
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(now_text)
}

fn codex_unix_timestamp_from_local_text(value: &str) -> i64 {
    parse_local_datetime_text(value)
        .map(|time| time.timestamp())
        .unwrap_or_else(|| Local::now().timestamp())
}

fn project_name_from_workspace_path(workspace_path: &str) -> String {
    Path::new(workspace_path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| workspace_path.to_string())
}

fn read_codex_session_index(
    codex_dir: &Path,
) -> Result<HashMap<String, (String, Option<String>)>, String> {
    let index_path = codex_dir.join("session_index.jsonl");
    if !index_path.exists() {
        return Ok(HashMap::new());
    }

    let file = fs::File::open(&index_path)
        .map_err(|error| format!("读取 Codex session_index.jsonl 失败：{}", error))?;
    let reader = BufReader::new(file);
    let mut index = HashMap::new();

    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取 Codex session 索引失败：{}", error))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let title = value
            .get("thread_name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("未命名 Codex 会话")
            .to_string();
        let updated_at = value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(|value| value.to_string());
        index.insert(id.to_string(), (title, updated_at));
    }

    Ok(index)
}

fn collect_codex_session_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }

    let entries =
        fs::read_dir(root).map_err(|error| format!("读取 Codex session 目录失败：{}", error))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取 Codex session 目录项失败：{}", error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_codex_session_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct ParsedCodexLocalSession {
    session_id: String,
    workspace_path: String,
    title: String,
    updated_at: String,
    created_at: String,
    message_count: i64,
    source_path: String,
    source: String,
    model_provider: String,
}

#[derive(Debug, Clone)]
struct SessionOwner {
    account_id: Option<i64>,
    profile_kind: String,
    profile_ref: String,
}

fn current_active_session_owner(connection: &Connection) -> Result<Option<SessionOwner>, String> {
    if let Some(profile) = query_credential_profiles(connection)?
        .into_iter()
        .find(|profile| profile.is_active)
    {
        if profile.profile_kind == "third_party_key" {
            return Ok(Some(SessionOwner {
                account_id: None,
                profile_kind: "third_party_key".to_string(),
                profile_ref: format!("key:{}", profile.id),
            }));
        }
        if profile.profile_kind == "official_account" {
            if let Some(account_id) = profile.linked_account_id {
                return Ok(Some(SessionOwner {
                    account_id: Some(account_id),
                    profile_kind: "official_account".to_string(),
                    profile_ref: format!("account:{}", account_id),
                }));
            }
        }
    }

    let accounts = query_accounts(connection)?;
    let account = accounts
        .iter()
        .find(|account| account.is_active && account.is_real_session)
        .or_else(|| {
            read_codex_auth_status_cached()
                .ok()
                .and_then(|status| {
                    active_account_id_for_runtime_login(connection, &status)
                        .ok()
                        .flatten()
                })
                .and_then(|account_id| accounts.iter().find(|account| account.id == account_id))
        })
        .or_else(|| {
            accounts
                .iter()
                .filter(|account| account.is_real_session)
                .max_by(|left, right| {
                    let left_time = left
                        .last_verified_at
                        .as_ref()
                        .or(left.last_check_time.as_ref());
                    let right_time = right
                        .last_verified_at
                        .as_ref()
                        .or(right.last_check_time.as_ref());
                    left_time.cmp(&right_time).then(left.id.cmp(&right.id))
                })
        });

    Ok(account.map(|account| SessionOwner {
        account_id: Some(account.id),
        profile_kind: "official_account".to_string(),
        profile_ref: format!("account:{}", account.id),
    }))
}

fn is_countable_codex_message(value: &Value) -> bool {
    let payload = value.get("payload").unwrap_or(&Value::Null);
    value.get("type").and_then(Value::as_str) == Some("response_item")
        && payload.get("type").and_then(Value::as_str) == Some("message")
}

fn compact_codex_title_text(text: &str) -> Option<String> {
    let without_environment = if let Some(end) = text.find("</environment_context>") {
        &text[end + "</environment_context>".len()..]
    } else {
        text
    };
    let compact = without_environment
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact.trim();
    if compact.is_empty() || compact == "<environment_context>" {
        return None;
    }

    let mut chars = compact.chars();
    let title = chars.by_ref().take(36).collect::<String>();
    if chars.next().is_some() {
        Some(format!("{title}..."))
    } else {
        Some(title)
    }
}

fn codex_message_text(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(&Value::Null);
    if value.get("type").and_then(Value::as_str) != Some("response_item")
        || payload.get("type").and_then(Value::as_str) != Some("message")
    {
        return None;
    }

    let content = payload.get("content")?;
    let parts = match content {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>(),
        Value::String(text) => vec![text.as_str()],
        _ => Vec::new(),
    };
    compact_codex_title_text(&parts.join(" "))
}

fn codex_session_title(
    indexed_title: Option<String>,
    first_user_title: Option<String>,
    workspace_path: &str,
    updated_at: Option<&str>,
) -> String {
    let indexed_title = indexed_title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "未命名 Codex 会话");
    if let Some(title) = indexed_title {
        return title;
    }
    if let Some(title) = first_user_title {
        return title;
    }

    let project_name = project_name_from_workspace_path(workspace_path);
    if let Some(updated_at) = updated_at {
        format!(
            "{} · {}",
            project_name,
            format_codex_timestamp(Some(updated_at))
        )
    } else {
        format!("{} · Codex 会话", project_name)
    }
}

fn parse_codex_local_session_file(
    path: &Path,
    index: &HashMap<String, (String, Option<String>)>,
) -> Result<Option<ParsedCodexLocalSession>, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("读取 Codex session 文件失败：{}", error))?;
    let reader = BufReader::new(file);
    let mut session_id: Option<String> = None;
    let mut workspace_path: Option<String> = None;
    let mut created_at: Option<String> = None;
    let mut latest_at: Option<String> = None;
    let mut first_user_title: Option<String> = None;
    let mut message_count = 0_i64;

    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取 Codex session 文件行失败：{}", error))?;
        if line.trim().is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) {
            latest_at = Some(timestamp.to_string());
        }

        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            let payload = value.get("payload").unwrap_or(&Value::Null);
            session_id = payload
                .get("id")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
                .or(session_id);
            workspace_path = payload
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_string())
                .or(workspace_path);
            created_at = payload
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
                .or(created_at);
        }

        if is_countable_codex_message(&value) {
            message_count += 1;
            let payload = value.get("payload").unwrap_or(&Value::Null);
            if first_user_title.is_none()
                && payload.get("role").and_then(Value::as_str) == Some("user")
            {
                first_user_title = codex_message_text(&value);
            }
        }
    }

    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let Some(workspace_path) = workspace_path else {
        return Ok(None);
    };

    let (indexed_title, indexed_updated_at) = index
        .get(&session_id)
        .cloned()
        .unwrap_or_else(|| ("未命名 Codex 会话".to_string(), None));
    let updated_at = indexed_updated_at
        .as_deref()
        .or(latest_at.as_deref())
        .or(created_at.as_deref());
    let created_at = created_at.as_deref().or(updated_at);
    let title = codex_session_title(
        Some(indexed_title),
        first_user_title,
        &workspace_path,
        updated_at,
    );

    Ok(Some(ParsedCodexLocalSession {
        session_id,
        workspace_path,
        title,
        updated_at: format_codex_timestamp(updated_at),
        created_at: format_codex_timestamp(created_at),
        message_count,
        source_path: path.to_string_lossy().to_string(),
        source: "vscode".to_string(),
        model_provider: "openai".to_string(),
    }))
}

fn count_codex_messages_in_rollout(path: &Path) -> i64 {
    let Ok(file) = fs::File::open(path) else {
        return 0;
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(is_countable_codex_message)
        .count() as i64
}

fn read_codex_state_threads(
    connection: &Connection,
    codex_dir: &Path,
) -> Result<Option<Vec<ParsedCodexLocalSession>>, String> {
    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(None);
    }

    let state = Connection::open(&state_path)
        .map_err(|error| format!("读取 Codex state_5.sqlite 失败：{}", error))?;
    let mut stmt = state
        .prepare(
            "SELECT id, title, cwd, model_provider, created_at, updated_at, rollout_path, source, archived
             FROM threads
             ORDER BY updated_at DESC, id DESC",
        )
        .map_err(|error| format!("读取 Codex threads 失败：{}", error))?;
    let rows = stmt
        .query_map([], |row| {
            let session_id: String = row.get(0)?;
            let raw_title: String = row.get(1)?;
            let workspace_path: String = row.get(2)?;
            let model_provider: String = row.get(3)?;
            let created_at: i64 = row.get(4)?;
            let updated_at: i64 = row.get(5)?;
            let rollout_path: String = row.get(6)?;
            let source: String = row.get(7)?;
            let title = raw_title.trim();
            Ok((
                ParsedCodexLocalSession {
                    session_id,
                    workspace_path: workspace_path.clone(),
                    title: if title.is_empty() {
                        codex_session_title(None, None, &workspace_path, None)
                    } else {
                        title.to_string()
                    },
                    updated_at: format_codex_unix_timestamp(updated_at),
                    created_at: format_codex_unix_timestamp(created_at),
                    message_count: 0,
                    source_path: rollout_path,
                    source,
                    model_provider,
                },
                row.get::<_, i64>(8)?,
            ))
        })
        .map_err(|error| format!("读取 Codex threads 行失败：{}", error))?;

    let mut sessions = Vec::new();
    for row in rows {
        let (mut session, archived) = row.map_err(|error| error.to_string())?;
        // Account isolation archives are still available in the Switcher library/import pool.
        // User archives remain excluded and are never unarchived just by listing candidates.
        if archived != 0
            && !is_codex_visibility_archive(
                connection,
                &session.session_id,
                &session.model_provider,
            )?
        {
            continue;
        }
        if !session.session_id.trim().is_empty()
            && !session.workspace_path.trim().is_empty()
            && Path::new(&session.source_path).is_file()
            && !is_internal_codex_review_title(&session.title)
        {
            session.message_count =
                count_codex_messages_in_rollout(Path::new(&session.source_path));
            sessions.push(session);
        }
    }
    Ok(Some(sessions))
}

fn is_main_codex_thread_source(source: &str) -> bool {
    source.trim() == "vscode"
}

fn read_codex_non_main_thread_ids(codex_dir: &Path) -> Result<Option<HashSet<String>>, String> {
    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(None);
    }

    let state = Connection::open(&state_path)
        .map_err(|error| format!("读取 Codex state_5.sqlite 失败：{}", error))?;
    let mut stmt = state
        .prepare("SELECT id FROM threads WHERE source != 'vscode'")
        .map_err(|error| format!("读取 Codex 非主会话 id 失败：{}", error))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取 Codex 非主会话 id 行失败：{}", error))?;
    Ok(Some(
        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(|error| error.to_string())?,
    ))
}

fn session_record_external_id(record: &SessionRecord) -> Option<String> {
    serde_json::from_str::<Value>(&record.raw_content)
        .ok()
        .and_then(|value| {
            value
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        })
}

fn is_main_codex_session_record(
    record: &SessionRecord,
    non_main_thread_ids: Option<&HashSet<String>>,
) -> bool {
    if record.record_type != "codex_imported" {
        return true;
    }
    let Some(non_main_thread_ids) = non_main_thread_ids else {
        return true;
    };
    // A saved history record may outlive its rollout/index entry. Only exclude
    // known non-main threads, not every record absent from the active sidebar.
    session_record_external_id(record)
        .map(|id| !non_main_thread_ids.contains(&id))
        .unwrap_or(true)
}

fn codex_writeback_model_provider(
    connection: &Connection,
    codex_dir: &Path,
    session: &ParsedCodexLocalSession,
    owner: &SessionOwner,
) -> String {
    codex_model_provider_for_owner(connection, codex_dir, owner, false)
        .unwrap_or_else(|| session.model_provider.clone())
}

fn key_profile_model_provider_from_ref(
    connection: &Connection,
    profile_ref: &str,
) -> Option<String> {
    let profile_id = profile_ref
        .strip_prefix("key:")
        .unwrap_or(profile_ref)
        .parse::<i64>()
        .ok()?;
    query_credential_profile_by_id(connection, profile_id)
        .ok()
        .map(|profile| key_profile_model_provider(&profile))
}

fn codex_model_provider_for_owner(
    connection: &Connection,
    codex_dir: &Path,
    owner: &SessionOwner,
    prefer_runtime_config: bool,
) -> Option<String> {
    match owner.profile_kind.as_str() {
        "official_account" => Some("openai".to_string()),
        "third_party_key" => {
            let runtime_provider = || read_codex_config_model_provider_from_dir(codex_dir);
            let profile_provider =
                || key_profile_model_provider_from_ref(connection, &owner.profile_ref);
            if prefer_runtime_config {
                runtime_provider()
                    .or_else(profile_provider)
                    .or_else(|| Some("custom".to_string()))
            } else {
                profile_provider()
                    .or_else(runtime_provider)
                    .or_else(|| Some("custom".to_string()))
            }
        }
        _ => None,
    }
}

fn sync_rollout_session_meta_model_provider(
    rollout_path: &Path,
    model_provider: &str,
) -> Result<(), String> {
    let content = fs::read_to_string(rollout_path)
        .map_err(|error| format!("读取 Codex rollout 元数据失败：{}", error))?;
    let had_trailing_newline = content.ends_with('\n');
    let mut changed = false;
    let mut updated_meta = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if updated_meta || line.trim().is_empty() {
            lines.push(line.to_string());
            continue;
        }

        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            lines.push(line.to_string());
            continue;
        };

        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            lines.push(line.to_string());
            continue;
        }

        updated_meta = true;
        if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
            let current = payload.get("model_provider").and_then(Value::as_str);
            if current != Some(model_provider) {
                payload.insert(
                    "model_provider".to_string(),
                    Value::String(model_provider.to_string()),
                );
                changed = true;
            }
        }

        if changed {
            let encoded = serde_json::to_string(&value)
                .map_err(|error| format!("写入 Codex rollout 元数据失败：{}", error))?;
            lines.push(encoded);
        } else {
            lines.push(line.to_string());
        }
    }

    if changed {
        let mut next_content = lines.join("\n");
        if had_trailing_newline {
            next_content.push('\n');
        }
        fs::write(rollout_path, next_content)
            .map_err(|error| format!("保存 Codex rollout 元数据失败：{}", error))?;
    }

    Ok(())
}

fn copy_rollout_for_imported_owner(
    codex_dir: &Path,
    session: &ParsedCodexLocalSession,
    owner: &SessionOwner,
    model_provider: &str,
) -> Result<ParsedCodexLocalSession, String> {
    let clone_key = format!(
        "codexswitcher:{}:{}:{}",
        session.session_id, owner.profile_kind, owner.profile_ref
    );
    let cloned_session_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, clone_key.as_bytes()).to_string();
    let target_path = canonical_codex_session_rollout_path(
        codex_dir,
        &cloned_session_id,
        codex_unix_timestamp_from_local_text(&session.created_at),
    );
    let target_dir = target_path
        .parent()
        .ok_or_else(|| "无法确定 Codex 导入会话目录。".to_string())?;
    fs::create_dir_all(&target_dir)
        .map_err(|error| format!("创建 Codex 导入会话目录失败：{}", error))?;

    let source_path = Path::new(&session.source_path);
    let content = fs::read_to_string(source_path)
        .map_err(|error| format!("读取待复制 Codex rollout 失败：{}", error))?;
    let had_trailing_newline = content.ends_with('\n');
    let mut updated_meta = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if updated_meta || line.trim().is_empty() {
            lines.push(line.to_string());
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            lines.push(line.to_string());
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            lines.push(line.to_string());
            continue;
        }
        let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
            return Err("Codex rollout 的 session_meta 缺少 payload。".to_string());
        };
        payload.insert("id".to_string(), Value::String(cloned_session_id.clone()));
        payload.insert(
            "model_provider".to_string(),
            Value::String(model_provider.to_string()),
        );
        lines.push(
            serde_json::to_string(&value)
                .map_err(|error| format!("生成 Codex 导入 rollout 失败：{}", error))?,
        );
        updated_meta = true;
    }

    if !updated_meta {
        return Err("待复制的 Codex rollout 缺少 session_meta，无法安全创建独立会话。".to_string());
    }
    let mut copied_content = lines.join("\n");
    if had_trailing_newline {
        copied_content.push('\n');
    }
    fs::write(&target_path, copied_content)
        .map_err(|error| format!("保存 Codex 导入 rollout 失败：{}", error))?;

    let mut cloned = session.clone();
    cloned.session_id = cloned_session_id;
    cloned.source_path = target_path.to_string_lossy().to_string();
    cloned.model_provider = model_provider.to_string();
    Ok(cloned)
}

fn codex_thread_sandbox_policy(workspace_path: &str) -> String {
    json!({
        "type": "workspace-write",
        "writable_roots": [workspace_path],
        "network_access": false,
        "exclude_tmpdir_env_var": false,
        "exclude_slash_tmp": false,
    })
    .to_string()
}

fn sqlite_table_has_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = ?1",
                table_name
            ),
            [column_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| error.to_string())
}

fn sqlite_table_exists(connection: &Connection, table_name: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| error.to_string())
}

fn first_user_message_from_rollout(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find_map(|value| {
            let payload = value.get("payload").unwrap_or(&Value::Null);
            if value.get("type").and_then(Value::as_str) == Some("response_item")
                && payload.get("type").and_then(Value::as_str) == Some("message")
                && payload.get("role").and_then(Value::as_str) == Some("user")
            {
                codex_message_text(&value)
            } else {
                None
            }
        })
}

fn codex_thread_preview(title: &str, first_user_message: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }

    first_user_message.trim().to_string()
}

fn refresh_codex_state_thread_visibility_metadata(
    state: &Connection,
    session_id: &str,
    preview: &str,
    updated_at: i64,
    updated_at_ms: i64,
) -> Result<(), String> {
    if sqlite_table_has_column(state, "threads", "thread_source")? {
        state
            .execute(
                "UPDATE threads
                 SET thread_source = COALESCE(NULLIF(thread_source, ''), 'user')
                 WHERE id = ?1",
                [session_id],
            )
            .map_err(|error| format!("更新 Codex thread 来源失败：{}", error))?;
    }

    if sqlite_table_has_column(state, "threads", "preview")? {
        state
            .execute(
                "UPDATE threads
                 SET preview = CASE
                    WHEN TRIM(COALESCE(preview, '')) = '' THEN ?1
                    ELSE preview
                 END
                 WHERE id = ?2",
                params![preview, session_id],
            )
            .map_err(|error| format!("更新 Codex thread 预览失败：{}", error))?;
    }

    if sqlite_table_has_column(state, "threads", "recency_at")? {
        state
            .execute(
                "UPDATE threads
                 SET recency_at = CASE
                    WHEN COALESCE(recency_at, 0) = 0 THEN ?1
                    ELSE recency_at
                 END
                 WHERE id = ?2",
                params![updated_at, session_id],
            )
            .map_err(|error| format!("更新 Codex thread 最近时间失败：{}", error))?;
    }

    if sqlite_table_has_column(state, "threads", "recency_at_ms")? {
        state
            .execute(
                "UPDATE threads
                 SET recency_at_ms = CASE
                    WHEN COALESCE(recency_at_ms, 0) = 0 THEN ?1
                    ELSE recency_at_ms
                 END
                 WHERE id = ?2",
                params![updated_at_ms, session_id],
            )
            .map_err(|error| format!("更新 Codex thread 最近时间毫秒失败：{}", error))?;
    }

    Ok(())
}

fn upsert_codex_state_thread_for_session(
    connection: &Connection,
    codex_dir: &Path,
    session: &ParsedCodexLocalSession,
    owner: &SessionOwner,
) -> Result<bool, String> {
    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(false);
    }

    let rollout_path = Path::new(&session.source_path);
    if !rollout_path.exists() {
        return Ok(false);
    }

    let state = Connection::open(&state_path)
        .map_err(|error| format!("写入 Codex state_5.sqlite 失败：{}", error))?;
    let created_at = codex_unix_timestamp_from_local_text(&session.created_at);
    let updated_at = codex_unix_timestamp_from_local_text(&session.updated_at);
    let created_at_ms = created_at * 1000;
    let updated_at_ms = updated_at * 1000;
    let sandbox_policy = codex_thread_sandbox_policy(&session.workspace_path);
    let first_user_message = first_user_message_from_rollout(rollout_path).unwrap_or_default();
    let preview = codex_thread_preview(&session.title, &first_user_message);
    let model_provider = codex_writeback_model_provider(connection, codex_dir, session, owner);
    sync_rollout_session_meta_model_provider(rollout_path, &model_provider)?;
    state
        .execute(
            "INSERT INTO threads
                (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                 sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                 archived_at, git_sha, git_branch, git_origin_url, cli_version,
                 first_user_message, agent_nickname, agent_role, memory_mode, model,
                 reasoning_effort, agent_path, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, 'vscode', ?5, ?6, ?7,
                     ?8, 'on-request', 0, 0, 0,
                     NULL, NULL, NULL, NULL, '',
                     ?9, NULL, NULL, 'enabled', NULL,
                     NULL, NULL, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                rollout_path = excluded.rollout_path,
                updated_at = excluded.updated_at,
                model_provider = excluded.model_provider,
                cwd = excluded.cwd,
                title = excluded.title,
                sandbox_policy = excluded.sandbox_policy,
                archived = 0,
                archived_at = NULL,
                first_user_message = CASE
                    WHEN threads.first_user_message = '' THEN excluded.first_user_message
                    ELSE threads.first_user_message
                END,
                updated_at_ms = excluded.updated_at_ms",
            params![
                session.session_id,
                session.source_path,
                created_at,
                updated_at,
                model_provider,
                session.workspace_path,
                session.title,
                sandbox_policy,
                first_user_message,
                created_at_ms,
                updated_at_ms,
            ],
        )
        .map_err(|error| format!("注册 Codex thread 失败：{}", error))?;

    refresh_codex_state_thread_visibility_metadata(
        &state,
        &session.session_id,
        &preview,
        updated_at,
        updated_at_ms,
    )?;

    Ok(true)
}

fn upsert_local_project_for_import(
    connection: &Connection,
    workspace_path: &str,
    last_active_at: &str,
) -> Result<i64, String> {
    let project_name = project_name_from_workspace_path(workspace_path);
    let existing_id = connection
        .query_row(
            "SELECT id FROM local_projects WHERE workspace_path = ?1 LIMIT 1",
            [workspace_path],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    if let Some(project_id) = existing_id {
        connection
            .execute(
                "UPDATE local_projects
                 SET name = ?1,
                     last_active_at = CASE
                        WHEN last_active_at IS NULL OR last_active_at < ?2 THEN ?2
                        ELSE last_active_at
                     END,
                     updated_at = ?2
                 WHERE id = ?3",
                params![project_name, last_active_at, project_id],
            )
            .map_err(|error| error.to_string())?;
        return Ok(project_id);
    }

    connection
        .execute(
            "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, ?3, ?3)",
            params![project_name, workspace_path, last_active_at],
        )
        .map_err(|error| error.to_string())?;
    Ok(connection.last_insert_rowid())
}

fn ensure_session_profile_link(
    connection: &Connection,
    session_id: i64,
    profile_kind: &str,
    profile_ref: &str,
) -> Result<(), String> {
    let exists = connection
        .query_row(
            "SELECT COUNT(*) FROM session_profile_links
             WHERE session_id = ?1 AND profile_kind = ?2 AND profile_ref = ?3",
            params![session_id, profile_kind, profile_ref],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    if exists == 0 {
        connection
            .execute(
                "INSERT INTO session_profile_links (session_id, profile_kind, profile_ref, access_mode, source_session_id, created_at)
                 VALUES (?1, ?2, ?3, 'owner', NULL, ?4)",
                params![session_id, profile_kind, profile_ref, now_text()],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn upsert_codex_imported_session(
    connection: &Connection,
    session: &ParsedCodexLocalSession,
    owner: &SessionOwner,
    owner_pinned: bool,
) -> Result<bool, String> {
    let project_id =
        upsert_local_project_for_import(connection, &session.workspace_path, &session.updated_at)?;
    let summary = format!(
        "Codex 本地会话 · {} 条消息 · {}",
        session.message_count, session.workspace_path
    );
    let raw_content = json!({
        "source": "codex_local_session",
        "session_id": session.session_id,
        "source_path": session.source_path,
        "workspace_path": session.workspace_path,
        "owner_pinned": owner_pinned,
    })
    .to_string();

    let existing_id = connection
        .query_row(
            "SELECT id FROM session_records
             WHERE record_type = 'codex_imported' AND external_session_id = ?1
             LIMIT 1",
            [&session.session_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let session_record_id = if let Some(record_id) = existing_id {
        connection
            .execute(
                "UPDATE session_records
                 SET project_id = ?1,
                     title = ?2,
                     summary = ?3,
                     raw_content = ?4,
                     message_count = ?5,
                     owner_account_id = ?6,
                     owner_profile_kind = ?7,
                     owner_profile_ref = ?8,
                     updated_at = ?9
                 WHERE id = ?10",
                params![
                    project_id,
                    session.title,
                    summary,
                    raw_content,
                    session.message_count,
                    owner.account_id,
                    owner.profile_kind.as_str(),
                    owner.profile_ref.as_str(),
                    session.updated_at,
                    record_id
                ],
            )
            .map_err(|error| error.to_string())?;
        record_id
    } else {
        connection
            .execute(
                "INSERT INTO session_records
                    (project_id, owner_account_id, owner_profile_kind, owner_profile_ref,
                     record_type, title, summary, raw_content, message_count,
                     source_record_id, external_session_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'codex_imported',
                         ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?11)",
                params![
                    project_id,
                    owner.account_id,
                    owner.profile_kind.as_str(),
                    owner.profile_ref.as_str(),
                    session.title,
                    summary,
                    raw_content,
                    session.message_count,
                    session.session_id,
                    session.created_at,
                    session.updated_at
                ],
            )
            .map_err(|error| error.to_string())?;
        connection.last_insert_rowid()
    };

    ensure_session_profile_link(
        connection,
        session_record_id,
        &owner.profile_kind,
        &owner.profile_ref,
    )?;
    Ok(existing_id.is_none())
}

struct CodexTitleBackfillCandidate {
    id: i64,
    raw_content: String,
    project_path: String,
    updated_at: String,
}

struct CodexOwnerBackfillCandidate {
    id: i64,
    owner_profile_kind: String,
    owner_profile_ref: String,
    raw_content: String,
}

fn is_default_codex_session_title(title: &str) -> bool {
    let title = title.trim();
    title.is_empty() || title == "未命名 Codex 会话" || title == "未命名会话"
}

fn is_internal_codex_review_title(title: &str) -> bool {
    title.trim_start().starts_with(
        "The following is the Codex agent history whose request action you are assessing.",
    )
}

fn codex_source_path_from_raw_content(raw_content: &str) -> Option<PathBuf> {
    serde_json::from_str::<Value>(raw_content)
        .ok()
        .and_then(|value| {
            value
                .get("source_path")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
        })
}

fn codex_owner_pinned_from_raw_content(raw_content: &str) -> bool {
    serde_json::from_str::<Value>(raw_content)
        .ok()
        .and_then(|value| value.get("owner_pinned").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn host_from_base_url(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed.split("://").nth(1).unwrap_or(trimmed);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn session_owner_from_identity_key(identity_key: &str) -> Option<SessionOwner> {
    if let Some(key_id) = identity_key.strip_prefix("key:") {
        return Some(SessionOwner {
            account_id: None,
            profile_kind: "third_party_key".to_string(),
            profile_ref: format!("key:{key_id}"),
        });
    }
    if let Some(account_id) = identity_key.strip_prefix("account:") {
        let parsed = account_id.parse::<i64>().ok()?;
        return Some(SessionOwner {
            account_id: Some(parsed),
            profile_kind: "official_account".to_string(),
            profile_ref: format!("account:{parsed}"),
        });
    }
    None
}

fn key_rollout_identity_hints(
    connection: &Connection,
) -> Result<Vec<(CodexCandidateIdentity, String)>, String> {
    Ok(query_credential_profiles(connection)?
        .into_iter()
        .filter(|profile| profile.profile_kind == "third_party_key")
        .filter_map(|profile| {
            let host = host_from_base_url(profile.base_url.as_deref()?);
            host.map(|host| {
                (
                    CodexCandidateIdentity {
                        key: format!("key:{}", profile.id),
                        label: profile.nickname,
                        kind_label: "Key".to_string(),
                    },
                    host,
                )
            })
        })
        .collect())
}

fn rollout_error_matches_host(message: &str, host: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains(host) && (normalized.contains("url:") || normalized.contains("url ("))
}

fn infer_custom_candidate_identity_from_source(
    connection: &Connection,
    source_path: &Path,
) -> Result<Option<CodexCandidateIdentity>, String> {
    if !source_path.exists() {
        return Ok(None);
    }

    let hints = key_rollout_identity_hints(connection)?;
    if hints.is_empty() {
        return Ok(None);
    }

    let file = match fs::File::open(source_path) {
        Ok(file) => file,
        Err(_) => return Ok(None),
    };
    let reader = BufReader::new(file);
    let mut earliest_hit: Option<(String, CodexCandidateIdentity)> = None;

    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取 Codex session 文件行失败：{}", error))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let payload = value.get("payload").unwrap_or(&Value::Null);
        if payload.get("type").and_then(Value::as_str) != Some("error") {
            continue;
        }
        let Some(message) = payload.get("message").and_then(Value::as_str) else {
            continue;
        };
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for (identity, host) in &hints {
            if !rollout_error_matches_host(message, host) {
                continue;
            }
            let replace_current = match earliest_hit.as_ref() {
                None => true,
                Some((_, _)) if timestamp.is_empty() => false,
                Some((current_timestamp, _)) => timestamp < current_timestamp.as_str(),
            };
            if replace_current {
                earliest_hit = Some((timestamp.to_string(), identity.clone()));
            }
            break;
        }
    }

    Ok(earliest_hit.map(|(_, identity)| identity))
}

fn codex_title_from_session_source(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = value.get("payload").unwrap_or(&Value::Null);
        if is_countable_codex_message(&value)
            && payload.get("role").and_then(Value::as_str) == Some("user")
        {
            if let Some(title) = codex_message_text(&value) {
                return Some(title);
            }
        }
    }
    None
}

fn backfill_codex_imported_session_titles(connection: &Connection) -> Result<(), String> {
    let candidates = {
        let mut stmt = connection
            .prepare(
                "SELECT s.id, s.raw_content, p.workspace_path, s.updated_at
                 FROM session_records s
                 JOIN local_projects p ON p.id = s.project_id
                 WHERE s.record_type = 'codex_imported'
                   AND (TRIM(s.title) = '' OR s.title = '未命名 Codex 会话' OR s.title = '未命名会话')
                 LIMIT 200",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CodexTitleBackfillCandidate {
                    id: row.get(0)?,
                    raw_content: row.get(1)?,
                    project_path: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(|error| error.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    for candidate in candidates {
        let title = codex_source_path_from_raw_content(&candidate.raw_content)
            .as_deref()
            .and_then(codex_title_from_session_source)
            .unwrap_or_else(|| {
                codex_session_title(
                    None,
                    None,
                    &candidate.project_path,
                    Some(&candidate.updated_at),
                )
            });
        if is_default_codex_session_title(&title) {
            continue;
        }
        connection
            .execute(
                "UPDATE session_records SET title = ?1 WHERE id = ?2",
                params![title, candidate.id],
            )
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn backfill_codex_imported_session_owners(connection: &Connection) -> Result<(), String> {
    let candidates = {
        let mut stmt = connection
            .prepare(
                "SELECT id, owner_profile_kind, owner_profile_ref, raw_content
                 FROM session_records
                 WHERE record_type = 'codex_imported'
                 LIMIT 300",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CodexOwnerBackfillCandidate {
                    id: row.get(0)?,
                    owner_profile_kind: row.get(1)?,
                    owner_profile_ref: row.get(2)?,
                    raw_content: row.get(3)?,
                })
            })
            .map_err(|error| error.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    for candidate in candidates {
        if codex_owner_pinned_from_raw_content(&candidate.raw_content) {
            continue;
        }
        let Some(source_path) = codex_source_path_from_raw_content(&candidate.raw_content) else {
            continue;
        };
        let Some(identity) =
            infer_custom_candidate_identity_from_source(connection, source_path.as_path())?
        else {
            continue;
        };
        let Some(owner) = session_owner_from_identity_key(&identity.key) else {
            continue;
        };
        if candidate.owner_profile_kind == owner.profile_kind
            && candidate.owner_profile_ref == owner.profile_ref
        {
            continue;
        }

        connection
            .execute(
                "UPDATE session_records
                 SET owner_account_id = ?1,
                     owner_profile_kind = ?2,
                     owner_profile_ref = ?3
                 WHERE id = ?4",
                params![
                    owner.account_id,
                    owner.profile_kind.as_str(),
                    owner.profile_ref.as_str(),
                    candidate.id
                ],
            )
            .map_err(|error| error.to_string())?;
        ensure_session_profile_link(
            connection,
            candidate.id,
            &owner.profile_kind,
            &owner.profile_ref,
        )?;
    }

    Ok(())
}

fn backfill_codex_imported_state_model_providers_for_dir(
    connection: &Connection,
    codex_dir: &Path,
) -> Result<(), String> {
    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(());
    }

    let candidates = {
        let mut stmt = connection
            .prepare(
                "SELECT owner_profile_kind, owner_profile_ref, external_session_id
                 FROM session_records
                 WHERE record_type = 'codex_imported'
                   AND owner_profile_kind IN ('official_account', 'third_party_key')
                   AND external_session_id IS NOT NULL
                   AND TRIM(external_session_id) != ''
                 LIMIT 300",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    if candidates.is_empty() {
        return Ok(());
    }

    let state = match Connection::open(&state_path) {
        Ok(state) => state,
        Err(_) => return Ok(()),
    };

    for (profile_kind, profile_ref, external_session_id) in candidates {
        let owner = SessionOwner {
            account_id: None,
            profile_kind,
            profile_ref,
        };
        let Some(expected_provider) =
            codex_model_provider_for_owner(connection, codex_dir, &owner, false)
        else {
            continue;
        };
        let thread = state
            .query_row(
                "SELECT model_provider, source, archived, rollout_path, title, updated_at, updated_at_ms
                 FROM threads
                 WHERE id = ?1",
                [external_session_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .optional();
        let Ok(Some((
            model_provider,
            source,
            _archived,
            rollout_path,
            title,
            updated_at,
            updated_at_ms,
        ))) = thread
        else {
            continue;
        };
        if !is_main_codex_thread_source(&source) {
            continue;
        }

        if model_provider != expected_provider
            && state
                .execute(
                    "UPDATE threads
                 SET model_provider = ?1
                 WHERE id = ?2 AND source = 'vscode'",
                    params![expected_provider, external_session_id],
                )
                .is_err()
        {
            continue;
        }

        let rollout_path = Path::new(&rollout_path);
        let first_user_message = first_user_message_from_rollout(rollout_path).unwrap_or_default();
        let preview = codex_thread_preview(&title, &first_user_message);
        if refresh_codex_state_thread_visibility_metadata(
            &state,
            &external_session_id,
            &preview,
            updated_at,
            updated_at_ms.unwrap_or(updated_at * 1000),
        )
        .is_err()
        {
            continue;
        }

        if rollout_path.exists() {
            let _ = sync_rollout_session_meta_model_provider(rollout_path, &expected_provider);
        }
    }

    Ok(())
}

fn backfill_codex_imported_state_model_providers(connection: &Connection) -> Result<(), String> {
    let Ok(codex_dir) = codex_config_dir() else {
        return Ok(());
    };
    backfill_codex_imported_state_model_providers_for_dir(connection, &codex_dir)
}

fn is_codex_visibility_archive(
    connection: &Connection,
    thread_id: &str,
    model_provider: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT 1 FROM codex_visibility_archives
             WHERE thread_id = ?1 AND model_provider = ?2",
            params![thread_id, model_provider],
            |_| Ok(()),
        )
        .optional()
        .map(|marker| marker.is_some())
        .map_err(|error| error.to_string())
}

fn mark_codex_visibility_archive(
    connection: &Connection,
    thread_id: &str,
    model_provider: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO codex_visibility_archives (thread_id, model_provider, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(thread_id) DO UPDATE SET
                model_provider = excluded.model_provider,
                created_at = excluded.created_at",
            params![thread_id, model_provider, now_text()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn clear_codex_visibility_archive(connection: &Connection, thread_id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM codex_visibility_archives WHERE thread_id = ?1",
            [thread_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Debug)]
struct CodexThreadArchiveAction {
    thread_id: String,
    method: &'static str,
}

fn canonical_codex_session_rollout_path(
    codex_dir: &Path,
    thread_id: &str,
    created_at: i64,
) -> PathBuf {
    let created_at = Local
        .timestamp_opt(created_at, 0)
        .single()
        .unwrap_or_else(Local::now);
    codex_dir
        .join("sessions")
        .join(created_at.format("%Y").to_string())
        .join(created_at.format("%m").to_string())
        .join(created_at.format("%d").to_string())
        .join(format!(
            "rollout-{}-{}.jsonl",
            created_at.format("%Y-%m-%dT%H-%M-%S"),
            thread_id
        ))
}

fn migrate_legacy_imported_rollout_path(
    state: &Connection,
    codex_dir: &Path,
    thread_id: &str,
    created_at: i64,
    rollout_path: PathBuf,
) -> Result<PathBuf, String> {
    let legacy_dir = codex_dir.join("sessions").join("codexswitcher-imported");
    if !rollout_path.starts_with(&legacy_dir) || !rollout_path.exists() {
        return Ok(rollout_path);
    }

    let target_path = canonical_codex_session_rollout_path(codex_dir, thread_id, created_at);
    if target_path == rollout_path {
        return Ok(rollout_path);
    }
    if target_path.exists() {
        return Err(format!(
            "迁移 Codex 导入会话 {} 失败：目标文件已存在 {}",
            thread_id,
            target_path.display()
        ));
    }

    let target_dir = target_path
        .parent()
        .ok_or_else(|| format!("无法确定 Codex 会话 {} 的目标目录", thread_id))?;
    fs::create_dir_all(target_dir)
        .map_err(|error| format!("创建 Codex 标准会话目录失败：{}", error))?;
    fs::rename(&rollout_path, &target_path)
        .map_err(|error| format!("迁移 Codex 导入会话文件失败：{}", error))?;

    if let Err(error) = state.execute(
        "UPDATE threads SET rollout_path = ?1 WHERE id = ?2 AND source = 'vscode'",
        params![target_path.to_string_lossy(), thread_id],
    ) {
        let _ = fs::rename(&target_path, &rollout_path);
        return Err(format!("更新 Codex 导入会话路径失败：{}", error));
    }

    Ok(target_path)
}

#[cfg(not(test))]
fn write_codex_app_server_message(stdin: &mut impl Write, message: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, message).map_err(|error| error.to_string())?;
    stdin.write_all(b"\n").map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())
}

#[cfg(not(test))]
fn wait_for_codex_app_server_response(
    receiver: &mpsc::Receiver<Value>,
    request_id: i64,
) -> Result<Value, String> {
    let deadline = Instant::now() + StdDuration::from_secs(12);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("Codex app-server 请求 {} 超时", request_id));
        }

        let message = receiver
            .recv_timeout(remaining)
            .map_err(|error| format!("等待 Codex app-server 响应失败：{}", error))?;
        if message.get("id").and_then(Value::as_i64) != Some(request_id) {
            continue;
        }

        if let Some(error) = message.get("error") {
            let detail = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(detail.to_string());
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
}

fn codex_thread_archive_action_is_satisfied(
    codex_dir: &Path,
    action: &CodexThreadArchiveAction,
) -> bool {
    let Ok(state) = Connection::open(codex_dir.join("state_5.sqlite")) else {
        return false;
    };
    let thread = state
        .query_row(
            "SELECT archived, rollout_path FROM threads WHERE id = ?1 AND source = 'vscode'",
            [action.thread_id.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional();
    let Ok(Some((archived, rollout_path))) = thread else {
        return false;
    };
    let rollout_path = PathBuf::from(rollout_path);
    let rollout_is_archived = rollout_path.starts_with(codex_dir.join("archived_sessions"));
    let rollout_exists = rollout_path.exists();

    match action.method {
        "thread/archive" => archived != 0 && rollout_is_archived && rollout_exists,
        "thread/unarchive" => archived == 0 && !rollout_is_archived && rollout_exists,
        _ => false,
    }
}

/// Use Codex's supported thread archive APIs instead of only toggling SQLite.
/// The app-server moves rollout files between `sessions` and
/// `archived_sessions`, which prevents the desktop app from rediscovering and
/// unhiding another identity's threads during startup.
fn apply_codex_thread_archive_actions(
    codex_dir: &Path,
    actions: &[CodexThreadArchiveAction],
) -> Result<(), String> {
    if actions.is_empty() {
        return Ok(());
    }

    #[cfg(test)]
    {
        apply_codex_thread_archive_actions_for_test(codex_dir, actions)
    }

    #[cfg(not(test))]
    {
        apply_codex_thread_archive_actions_via_server(codex_dir, actions)
    }
}

#[cfg(test)]
fn apply_codex_thread_archive_actions_for_test(
    codex_dir: &Path,
    actions: &[CodexThreadArchiveAction],
) -> Result<(), String> {
    let state_path = codex_dir.join("state_5.sqlite");
    let state = Connection::open(&state_path)
        .map_err(|error| format!("打开测试 Codex thread 索引失败：{}", error))?;
    for action in actions {
        let current_path = state
            .query_row(
                "SELECT rollout_path FROM threads WHERE id = ?1 AND source = 'vscode'",
                [action.thread_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .map(PathBuf::from)
            .map_err(|error| error.to_string())?;
        let file_name = current_path
            .file_name()
            .ok_or_else(|| format!("测试会话 {} 缺少 rollout 文件名", action.thread_id))?;
        let (target_path, archived) = match action.method {
            "thread/archive" => (codex_dir.join("archived_sessions").join(file_name), 1),
            "thread/unarchive" => (codex_dir.join("sessions").join(file_name), 0),
            method => return Err(format!("未知测试会话操作：{}", method)),
        };
        if current_path != target_path {
            fs::create_dir_all(
                target_path
                    .parent()
                    .ok_or_else(|| "测试 rollout 目标目录不存在".to_string())?,
            )
            .map_err(|error| error.to_string())?;
            if target_path.exists() {
                fs::remove_file(&target_path).map_err(|error| error.to_string())?;
            }
            fs::rename(&current_path, &target_path).map_err(|error| error.to_string())?;
        }
        state
            .execute(
                "UPDATE threads
                 SET archived = ?1,
                     archived_at = CASE WHEN ?1 = 1 THEN strftime('%s', 'now') ELSE NULL END,
                     rollout_path = ?2
                 WHERE id = ?3 AND source = 'vscode'",
                params![archived, target_path.to_string_lossy(), action.thread_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(test))]
fn apply_codex_thread_archive_actions_via_server(
    codex_dir: &Path,
    actions: &[CodexThreadArchiveAction],
) -> Result<(), String> {
    let codex_cli = resolve_codex_cli_path()?;
    let mut child = Command::new(codex_cli)
        .args(["app-server", "--stdio"])
        .env("CODEX_HOME", codex_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 Codex app-server 失败：{}", error))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法连接 Codex app-server 输入流".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法连接 Codex app-server 输出流".to_string())?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        }
    });

    let result = (|| {
        write_codex_app_server_message(
            &mut stdin,
            &json!({
                "method": "initialize",
                "id": 1,
                "params": {
                    "clientInfo": {
                        "name": "codex_switcher",
                        "title": "Codex Switcher",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
        )?;
        wait_for_codex_app_server_response(&receiver, 1)?;
        write_codex_app_server_message(
            &mut stdin,
            &json!({ "method": "initialized", "params": {} }),
        )?;

        let mut action_errors = Vec::new();
        for (index, action) in actions.iter().enumerate() {
            let request_id = index as i64 + 2;
            write_codex_app_server_message(
                &mut stdin,
                &json!({
                    "method": action.method,
                    "id": request_id,
                    "params": { "threadId": action.thread_id }
                }),
            )?;
            if let Err(error) = wait_for_codex_app_server_response(&receiver, request_id) {
                // Codex may report a stale response after another process has
                // already completed the move. Re-read the index before
                // deciding that the action failed.
                if codex_thread_archive_action_is_satisfied(codex_dir, action) {
                    continue;
                }

                // The desktop process owns active thread writers. Identity
                // switching restarts it and runs visibility sync again after
                // those writers have stopped, so this is a retryable state.
                if error.contains("already has an active writer") {
                    continue;
                }

                action_errors.push(format!(
                    "{} 会话 {} 失败：{}",
                    if action.method == "thread/archive" {
                        "归档"
                    } else {
                        "恢复"
                    },
                    action.thread_id,
                    error
                ));
            }
        }

        if !action_errors.is_empty() {
            return Err(action_errors.join("；"));
        }
        Ok(())
    })();

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn reconcile_codex_visibility_archive_actions(
    connection: &Connection,
    codex_dir: &Path,
    actions: &[CodexThreadArchiveAction],
) -> Result<(), String> {
    for action in actions {
        if action.method == "thread/unarchive"
            && codex_thread_archive_action_is_satisfied(codex_dir, action)
        {
            clear_codex_visibility_archive(connection, &action.thread_id)?;
        }
    }
    Ok(())
}

fn purge_missing_codex_threads_from_state(state_path: &Path) -> Result<usize, String> {
    if !state_path.exists() {
        return Ok(0);
    }

    let mut state = Connection::open(state_path)
        .map_err(|error| format!("打开 Codex thread 索引失败：{}", error))?;
    state
        .busy_timeout(StdDuration::from_secs(5))
        .map_err(|error| format!("等待 Codex thread 索引失败：{}", error))?;
    state
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| format!("启用 Codex thread 索引外键失败：{}", error))?;

    let missing_thread_ids = {
        let mut stmt = state
            .prepare("SELECT id, rollout_path FROM threads WHERE source = 'vscode'")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        rows.filter_map(|row| row.ok())
            .filter_map(|(thread_id, rollout_path)| {
                (!Path::new(&rollout_path).exists()).then_some(thread_id)
            })
            .collect::<Vec<_>>()
    };
    if missing_thread_ids.is_empty() {
        return Ok(0);
    }

    let has_spawn_edges = sqlite_table_exists(&state, "thread_spawn_edges")?;
    let transaction = state
        .transaction()
        .map_err(|error| format!("开始清理 Codex thread 索引失败：{}", error))?;
    for thread_id in &missing_thread_ids {
        if has_spawn_edges {
            transaction
                .execute(
                    "DELETE FROM thread_spawn_edges
                     WHERE parent_thread_id = ?1 OR child_thread_id = ?1",
                    [thread_id],
                )
                .map_err(|error| format!("清理 Codex thread 关联索引失败：{}", error))?;
        }
        transaction
            .execute("DELETE FROM threads WHERE id = ?1", [thread_id])
            .map_err(|error| format!("清理无文件 Codex thread 失败：{}", error))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交 Codex thread 索引清理失败：{}", error))?;
    Ok(missing_thread_ids.len())
}

fn purge_missing_codex_thread_indexes(
    codex_dir: &Path,
    include_primary: bool,
) -> Result<usize, String> {
    let primary_state = codex_dir.join("state_5.sqlite");
    let legacy_state = codex_dir.join("sqlite").join("state_5.sqlite");
    let mut removed = 0;
    if include_primary {
        removed += purge_missing_codex_threads_from_state(&primary_state)?;
    }
    if legacy_state != primary_state {
        removed += purge_missing_codex_threads_from_state(&legacy_state)?;
    }
    Ok(removed)
}

fn purge_missing_primary_codex_threads_with_cli(codex_dir: &Path) -> Result<usize, String> {
    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(0);
    }
    let state = Connection::open(&state_path)
        .map_err(|error| format!("读取 Codex 主 thread 索引失败：{}", error))?;
    state
        .busy_timeout(StdDuration::from_secs(5))
        .map_err(|error| format!("等待 Codex 主 thread 索引失败：{}", error))?;
    let missing_thread_ids = {
        let mut stmt = state
            .prepare("SELECT id, rollout_path FROM threads WHERE source = 'vscode'")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        rows.filter_map(|row| row.ok())
            .filter_map(|(thread_id, rollout_path)| {
                (!Path::new(&rollout_path).exists()).then_some(thread_id)
            })
            .collect::<Vec<_>>()
    };
    drop(state);

    if missing_thread_ids.is_empty() {
        return Ok(0);
    }
    let codex_cli = resolve_codex_cli_path()?;
    let mut removed = 0;
    let mut errors = Vec::new();
    for thread_id in missing_thread_ids {
        let result = Command::new(&codex_cli)
            .args(["delete", "--force", thread_id.as_str()])
            .env("CODEX_HOME", codex_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        match result {
            Ok(output) if output.status.success() => removed += 1,
            Ok(output) => errors.push(format!(
                "{}：{}",
                thread_id,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => errors.push(format!("{}：{}", thread_id, error)),
        }
    }
    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(format!("清理无文件 Codex 会话失败：{}", errors.join("；")))
    }
}

fn sync_codex_local_thread_catalog(codex_dir: &Path) -> Result<usize, String> {
    let state_path = codex_dir.join("state_5.sqlite");
    let catalog_path = codex_dir.join("sqlite").join("codex-dev.db");
    if !state_path.exists() || !catalog_path.exists() {
        return Ok(0);
    }

    let state = Connection::open(&state_path)
        .map_err(|error| format!("读取 Codex 主 thread 索引失败：{}", error))?;
    state
        .busy_timeout(StdDuration::from_secs(5))
        .map_err(|error| format!("等待 Codex 主 thread 索引失败：{}", error))?;
    let visible_threads = {
        let mut stmt = state
            .prepare(
                "SELECT id, title, created_at, updated_at, cwd, source, model_provider,
                        git_branch, thread_source, recency_at, project_id, rollout_path
                 FROM threads
                 WHERE source = 'vscode' AND archived = 0",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, String>(11)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        rows.filter_map(|row| row.ok())
            .filter(|row| Path::new(&row.11).exists())
            .collect::<Vec<_>>()
    };
    drop(state);

    let mut catalog = Connection::open(&catalog_path)
        .map_err(|error| format!("读取 Codex 本地会话目录失败：{}", error))?;
    catalog
        .busy_timeout(StdDuration::from_secs(5))
        .map_err(|error| format!("等待 Codex 本地会话目录失败：{}", error))?;
    if !sqlite_table_exists(&catalog, "local_thread_catalog")? {
        return Ok(0);
    }
    let observation_sequence = catalog
        .query_row(
            "SELECT COALESCE(MAX(observation_sequence), 0) + 1 FROM local_thread_catalog",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(1);
    let transaction = catalog
        .transaction()
        .map_err(|error| format!("开始更新 Codex 本地会话目录失败：{}", error))?;
    let removed = transaction
        .execute(
            "DELETE FROM local_thread_catalog
             WHERE host_id = 'local' AND source_kind = 'vscode'",
            [],
        )
        .map_err(|error| format!("清理 Codex 本地会话目录失败：{}", error))?;
    for thread in visible_threads {
        transaction
            .execute(
                "INSERT INTO local_thread_catalog
                    (host_id, thread_id, display_title, source_created_at, source_updated_at,
                     cwd, source_kind, source_detail, model_provider, git_branch,
                     observation_sequence, missing_candidate, thread_source, source_recency_at,
                     pending_observed_title, project_id, conversation_origin)
                 VALUES ('local', ?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, 0, ?10, ?11, 0, ?12, NULL)",
                params![
                    thread.0,
                    thread.1,
                    thread.2,
                    thread.3,
                    thread.4,
                    thread.5,
                    thread.6,
                    thread.7,
                    observation_sequence,
                    thread.8,
                    thread.9,
                    thread.10,
                ],
            )
            .map_err(|error| format!("写入 Codex 本地会话目录失败：{}", error))?;
    }
    if sqlite_table_exists(&transaction, "local_thread_catalog_metadata")? {
        transaction
            .execute(
                "UPDATE local_thread_catalog_metadata
                 SET catalog_revision = catalog_revision + 1 WHERE id = 1",
                [],
            )
            .map_err(|error| format!("刷新 Codex 会话目录版本失败：{}", error))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("提交 Codex 本地会话目录失败：{}", error))?;
    Ok(removed)
}

/// Codex has one global `state_5.sqlite` and one active `config.toml`.  Keep
/// only threads that can be resolved by the active identity visible in that
/// shared index; otherwise Codex renders stale threads but cannot open them
/// because their provider is absent from the current config.
fn sync_codex_thread_visibility_for_owner(
    connection: &Connection,
    codex_dir: &Path,
    owner: &SessionOwner,
) -> Result<(), String> {
    // Some Codex versions leave an older index at `sqlite/state_5.sqlite`.
    // The desktop app imports that index again at startup, so stale rows with
    // deleted rollout files must be removed from both locations. Never mutate
    // the active primary index while the desktop process is running.
    let desktop_running = !cfg!(test) && codex_desktop_app_is_running();
    purge_missing_codex_thread_indexes(codex_dir, !desktop_running)?;
    if desktop_running {
        purge_missing_primary_codex_threads_with_cli(codex_dir)?;
    }

    let state_path = codex_dir.join("state_5.sqlite");
    if !state_path.exists() {
        return Ok(());
    }

    let expected_provider = codex_model_provider_for_owner(connection, codex_dir, owner, false);
    let visible_imported_ids = {
        let mut stmt = connection
            .prepare(
                "SELECT external_session_id
                 FROM session_records
                 WHERE record_type = 'codex_imported'
                   AND owner_profile_kind = ?1
                   AND owner_profile_ref = ?2
                   AND external_session_id IS NOT NULL
                   AND TRIM(external_session_id) != ''",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![owner.profile_kind, owner.profile_ref], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(|error| error.to_string())?
    };

    let legacy_provider = if owner.profile_kind == "third_party_key" {
        let profile_id = owner
            .profile_ref
            .strip_prefix("key:")
            .unwrap_or(&owner.profile_ref)
            .parse::<i64>()
            .ok();
        profile_id.and_then(|profile_id| {
            let profile = query_credential_profile_by_id(connection, profile_id).ok()?;
            let same_provider_count = query_credential_profiles(connection)
                .ok()?
                .into_iter()
                .filter(|candidate| {
                    candidate.profile_kind == "third_party_key"
                        && candidate
                            .provider
                            .trim()
                            .eq_ignore_ascii_case(profile.provider.trim())
                })
                .count();
            (same_provider_count == 1).then(|| profile.provider.trim().to_string())
        })
    } else {
        None
    };

    let state = Connection::open(&state_path)
        .map_err(|error| format!("更新 Codex thread 可见性失败：{}", error))?;
    state
        .busy_timeout(StdDuration::from_secs(5))
        .map_err(|error| format!("等待 Codex thread 数据库失败：{}", error))?;
    let mut stmt = state
        .prepare(
            "SELECT id, model_provider, archived, rollout_path, created_at
             FROM threads WHERE source = 'vscode'",
        )
        .map_err(|error| error.to_string())?;
    let threads = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(stmt);

    let archived_sessions_dir = codex_dir.join("archived_sessions");
    let mut archive_actions = Vec::new();
    for (thread_id, model_provider, archived, rollout_path, created_at) in threads {
        let is_current_official = owner.profile_kind == "official_account"
            && is_official_codex_model_provider(&model_provider);
        let is_current_key = expected_provider.as_deref() == Some(model_provider.as_str())
            || visible_imported_ids.contains(&thread_id);
        let is_unambiguous_legacy_key = legacy_provider.as_deref() == Some(model_provider.trim());
        let visible = is_current_official || is_current_key || is_unambiguous_legacy_key;

        let rollout_path = migrate_legacy_imported_rollout_path(
            &state,
            codex_dir,
            &thread_id,
            created_at,
            PathBuf::from(rollout_path),
        )?;
        let rollout_exists = rollout_path.exists();
        let rollout_is_archived = rollout_path.starts_with(&archived_sessions_dir);
        let switcher_archived =
            is_codex_visibility_archive(connection, &thread_id, &model_provider)?;

        if visible {
            if let Some(expected_provider) = expected_provider.as_deref() {
                if owner.profile_kind == "third_party_key"
                    && (visible_imported_ids.contains(&thread_id) || is_unambiguous_legacy_key)
                    && model_provider != expected_provider
                {
                    state
                        .execute(
                            "UPDATE threads
                             SET model_provider = ?1
                             WHERE id = ?2 AND source = 'vscode'",
                            params![expected_provider, thread_id],
                        )
                        .map_err(|error| error.to_string())?;
                }
            }

            if !rollout_exists {
                // A stale index row without its rollout cannot be opened. Do
                // not surface it merely because its provider matches the
                // active identity; that recreates the disabled sidebar rows
                // this visibility sync is intended to remove.
                state
                    .execute(
                        "UPDATE threads
                         SET archived = 1,
                             archived_at = CASE WHEN archived = 0 THEN strftime('%s', 'now') ELSE archived_at END
                         WHERE id = ?1 AND source = 'vscode'",
                        [thread_id.as_str()],
                    )
                    .map_err(|error| error.to_string())?;
                clear_codex_visibility_archive(connection, &thread_id)?;
            } else if rollout_is_archived && switcher_archived {
                archive_actions.push(CodexThreadArchiveAction {
                    thread_id,
                    method: "thread/unarchive",
                });
            } else if rollout_is_archived {
                // This thread was archived by the user or by Codex itself,
                // rather than by identity isolation. Keep it archived even
                // when its owner becomes active again.
                state
                    .execute(
                        "UPDATE threads
                         SET archived = 1,
                             archived_at = COALESCE(archived_at, strftime('%s', 'now'))
                         WHERE id = ?1 AND source = 'vscode'",
                        [thread_id.as_str()],
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                state
                    .execute(
                        "UPDATE threads SET archived = 0, archived_at = NULL
                         WHERE id = ?1 AND source = 'vscode'",
                        [thread_id.as_str()],
                    )
                    .map_err(|error| error.to_string())?;
                clear_codex_visibility_archive(connection, &thread_id)?;
            }
        } else {
            if rollout_exists && !rollout_is_archived {
                // Call the supported Codex archive API even when an older
                // Switcher build already set archived=1 directly. The file
                // must also leave `sessions`, otherwise paginated history is
                // rediscovered during the next desktop startup.
                mark_codex_visibility_archive(connection, &thread_id, &model_provider)?;
                archive_actions.push(CodexThreadArchiveAction {
                    thread_id,
                    method: "thread/archive",
                });
            } else if archived == 0 || rollout_is_archived || !rollout_exists {
                state
                    .execute(
                        "UPDATE threads
                         SET archived = 1,
                             archived_at = CASE WHEN archived = 0 THEN strftime('%s', 'now') ELSE archived_at END
                         WHERE id = ?1 AND source = 'vscode'",
                        [thread_id.as_str()],
                    )
                    .map_err(|error| error.to_string())?;
                if !rollout_exists {
                    clear_codex_visibility_archive(connection, &thread_id)?;
                }
            }
        }
    }

    drop(state);
    let action_result = apply_codex_thread_archive_actions(codex_dir, &archive_actions);
    let reconcile_result =
        reconcile_codex_visibility_archive_actions(connection, codex_dir, &archive_actions);
    let catalog_result = sync_codex_local_thread_catalog(codex_dir).map(|_| ());
    action_result.and(reconcile_result).and(catalog_result)
}

fn sync_codex_thread_visibility_for_active_owner(connection: &Connection) -> Result<(), String> {
    let Some(owner) = current_active_session_owner(connection)? else {
        return Ok(());
    };
    let codex_dir = codex_config_dir()?;
    sync_codex_thread_visibility_for_owner(connection, &codex_dir, &owner)
}

#[cfg(target_os = "macos")]
fn codex_desktop_app_is_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "ChatGPT"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn codex_desktop_app_is_running() -> bool {
    false
}

/// Codex keeps the active provider and sidebar thread list in process memory.
/// Reload it after an identity change so the newly written config and archived
/// flags take effect together. The delay lets the Tauri invoke return first.
fn schedule_codex_desktop_reload_if_running(app: &tauri::AppHandle) {
    if cfg!(test) || !codex_desktop_app_is_running() {
        return;
    }

    let app_handle = app.clone();
    thread::spawn(move || {
        thread::sleep(StdDuration::from_millis(700));

        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("osascript")
                .args(["-e", "tell application id \"com.openai.codex\" to quit"])
                .status();

            for _ in 0..20 {
                if !codex_desktop_app_is_running() {
                    break;
                }
                thread::sleep(StdDuration::from_millis(250));
            }

            if codex_desktop_app_is_running() {
                let _ = Command::new("pkill")
                    .args(["-TERM", "-x", "ChatGPT"])
                    .status();
                thread::sleep(StdDuration::from_millis(500));
            }

            if let Some(state) = app_handle.try_state::<AppState>() {
                if let Ok(connection) = state.db.lock() {
                    let _ = sync_codex_thread_visibility_for_active_owner(&connection);
                }
            }

            let _ = Command::new("open")
                .args(["-b", "com.openai.codex"])
                .status();
        }
    });
}

fn import_codex_local_sessions_from_dir(
    connection: &Connection,
    codex_dir: &Path,
) -> Result<CodexLocalSessionImportResult, String> {
    let index = read_codex_session_index(codex_dir)?;
    let candidate_ids = index.keys().cloned().collect::<Vec<_>>();
    import_codex_local_session_candidates_from_dir(connection, codex_dir, &candidate_ids, false)
}

fn imported_codex_session_owner(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<(i64, String, String)>, String> {
    connection
        .query_row(
            "SELECT id, owner_profile_kind, owner_profile_ref
             FROM session_records
             WHERE record_type = 'codex_imported' AND external_session_id = ?1
             LIMIT 1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
struct CodexCandidateIdentity {
    key: String,
    label: String,
    kind_label: String,
}

fn identity_from_owner_ref(
    connection: &Connection,
    profile_kind: &str,
    profile_ref: &str,
) -> Result<CodexCandidateIdentity, String> {
    if profile_kind == "third_party_key" {
        let profile_id = profile_ref
            .strip_prefix("key:")
            .unwrap_or(profile_ref)
            .parse::<i64>()
            .ok();
        let label = profile_id
            .and_then(|id| query_credential_profile_by_id(connection, id).ok())
            .map(|profile| profile.nickname)
            .unwrap_or_else(|| profile_ref.to_string());
        return Ok(CodexCandidateIdentity {
            key: if profile_ref.starts_with("key:") {
                profile_ref.to_string()
            } else {
                format!("key:{}", profile_ref)
            },
            label,
            kind_label: "Key".to_string(),
        });
    }

    if profile_kind == "official_account" {
        let account_id = profile_ref
            .strip_prefix("account:")
            .unwrap_or(profile_ref)
            .parse::<i64>()
            .ok();
        let label = account_id
            .and_then(|id| query_account_by_id(connection, id).ok())
            .map(|account| account.nickname)
            .unwrap_or_else(|| profile_ref.to_string());
        return Ok(CodexCandidateIdentity {
            key: account_id
                .map(|id| format!("account:{}", id))
                .unwrap_or_else(|| profile_ref.to_string()),
            label,
            kind_label: "官方账号".to_string(),
        });
    }

    Ok(CodexCandidateIdentity {
        key: format!("{}:{}", profile_kind, profile_ref),
        label: profile_ref.to_string(),
        kind_label: "其他".to_string(),
    })
}

fn active_official_candidate_identity(
    connection: &Connection,
) -> Result<Option<CodexCandidateIdentity>, String> {
    let account = query_accounts(connection)?
        .into_iter()
        .find(|account| account.is_active && account.is_real_session);
    Ok(account.map(|account| CodexCandidateIdentity {
        key: format!("account:{}", account.id),
        label: account.nickname,
        kind_label: "官方账号".to_string(),
    }))
}

fn is_official_codex_model_provider(model_provider: &str) -> bool {
    model_provider.trim().eq_ignore_ascii_case("openai")
}

fn candidate_identity_for_session(
    connection: &Connection,
    session: &ParsedCodexLocalSession,
    imported: Option<&(i64, String, String)>,
) -> Result<CodexCandidateIdentity, String> {
    if session.source_path.contains("/codexswitcher-imported/") {
        if let Some((_, profile_kind, profile_ref)) = imported {
            return identity_from_owner_ref(connection, profile_kind, profile_ref);
        }
    }

    // Scoped providers encode the exact Key, including when several Keys share a host.
    if let Some(profile_id) = session
        .model_provider
        .trim()
        .strip_prefix("codexswitcher-key-")
    {
        if let Ok(profile_id) = profile_id.parse::<i64>() {
            return identity_from_owner_ref(
                connection,
                "third_party_key",
                &format!("key:{profile_id}"),
            );
        }
    }

    if !is_official_codex_model_provider(&session.model_provider) {
        if let Some(identity) = infer_custom_candidate_identity_from_source(
            connection,
            Path::new(&session.source_path),
        )? {
            return Ok(identity);
        }
    }

    if let Some((_, profile_kind, profile_ref)) = imported {
        if profile_kind != "local_codex" {
            return identity_from_owner_ref(connection, profile_kind, profile_ref);
        }
    }

    if is_official_codex_model_provider(&session.model_provider) {
        if let Some(identity) = active_official_candidate_identity(connection)? {
            return Ok(identity);
        }
    } else {
        let label = if session.model_provider.trim().is_empty() {
            "本地自定义 Key".to_string()
        } else {
            format!("本地 {}", session.model_provider.trim())
        };
        return Ok(CodexCandidateIdentity {
            key: format!("codex_provider:{}", session.model_provider),
            label,
            kind_label: "Key".to_string(),
        });
    }

    if let Some(identity) = active_official_candidate_identity(connection)? {
        return Ok(identity);
    }

    Ok(CodexCandidateIdentity {
        key: format!("codex_provider:{}", session.model_provider),
        label: "本地 OpenAI".to_string(),
        kind_label: "官方账号".to_string(),
    })
}

fn list_codex_local_session_candidates_from_dir(
    connection: &Connection,
    codex_dir: &Path,
) -> Result<Vec<CodexLocalSessionCandidate>, String> {
    let sessions = if let Some(sessions) = read_codex_state_threads(connection, codex_dir)? {
        sessions
    } else {
        let index = read_codex_session_index(codex_dir)?;
        let mut files = Vec::new();
        collect_codex_session_files(&codex_dir.join("sessions"), &mut files)?;
        files.sort();

        let indexed_ids = index.keys().cloned().collect::<HashSet<_>>();
        let mut sessions = Vec::new();
        for file in files {
            let Some(session) = parse_codex_local_session_file(&file, &index)? else {
                continue;
            };
            if !indexed_ids.contains(&session.session_id) {
                continue;
            }
            sessions.push(session);
        }
        sessions
    };

    let mut candidates = Vec::new();
    let mut seen_ids = HashSet::new();
    for session in sessions {
        if is_internal_codex_review_title(&session.title) {
            continue;
        }
        if !is_main_codex_thread_source(&session.source) {
            continue;
        }
        if !seen_ids.insert(session.session_id.clone()) {
            continue;
        }
        let imported = imported_codex_session_owner(connection, &session.session_id)?;
        let identity = candidate_identity_for_session(connection, &session, imported.as_ref())?;
        candidates.push(CodexLocalSessionCandidate {
            candidate_id: session.session_id.clone(),
            identity_key: identity.key,
            identity_label: identity.label,
            identity_kind_label: identity.kind_label,
            project_name: project_name_from_workspace_path(&session.workspace_path),
            project_path: session.workspace_path,
            title: session.title,
            message_count: session.message_count,
            source_path: session.source_path,
            created_at: session.created_at,
            updated_at: session.updated_at,
            imported_session_id: imported.as_ref().map(|value| value.0),
            imported_owner_profile_kind: imported.as_ref().map(|value| value.1.clone()),
            imported_owner_profile_ref: imported.as_ref().map(|value| value.2.clone()),
        });
    }

    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(candidates)
}

fn session_for_import_owner(
    connection: &Connection,
    codex_dir: &Path,
    session: &ParsedCodexLocalSession,
    owner: &SessionOwner,
) -> Result<ParsedCodexLocalSession, String> {
    if owner.profile_kind != "third_party_key" {
        return Ok(session.clone());
    }

    let imported = imported_codex_session_owner(connection, &session.session_id)?;
    let source_identity = candidate_identity_for_session(connection, session, imported.as_ref())?;
    let target_provider = codex_model_provider_for_owner(connection, codex_dir, owner, false)
        .unwrap_or_else(|| "custom".to_string());
    let belongs_to_other_key = if source_identity.key.starts_with("key:") {
        source_identity.key != owner.profile_ref
    } else if source_identity.key.starts_with("codex_provider:") {
        let matching_profiles = query_credential_profiles(connection)?
            .into_iter()
            .filter(|profile| {
                profile.profile_kind == "third_party_key"
                    && key_profile_model_provider(profile)
                        .eq_ignore_ascii_case(session.model_provider.trim())
            })
            .collect::<Vec<_>>();
        matching_profiles.len() == 1
            && format!("key:{}", matching_profiles[0].id) != owner.profile_ref
    } else {
        false
    };

    if !belongs_to_other_key {
        return Ok(session.clone());
    }

    copy_rollout_for_imported_owner(codex_dir, session, owner, &target_provider)
}

fn import_codex_local_session_candidates_from_dir(
    connection: &Connection,
    codex_dir: &Path,
    candidate_ids: &[String],
    owner_pinned: bool,
) -> Result<CodexLocalSessionImportResult, String> {
    let owner = current_active_session_owner(connection)?.unwrap_or_else(|| SessionOwner {
        account_id: None,
        profile_kind: "local_codex".to_string(),
        profile_ref: "local".to_string(),
    });
    let requested_ids = candidate_ids.iter().cloned().collect::<HashSet<_>>();
    if let Some(sessions) = read_codex_state_threads(connection, codex_dir)? {
        let mut scanned_files = 0_i64;
        let mut imported_sessions = 0_i64;
        let mut updated_sessions = 0_i64;
        let mut skipped_files = 0_i64;
        let mut codex_synced_threads = 0_i64;
        let mut codex_skipped_threads = 0_i64;

        for session in sessions {
            scanned_files += 1;
            if is_internal_codex_review_title(&session.title) {
                skipped_files += 1;
                continue;
            }
            if !requested_ids.contains(&session.session_id) {
                skipped_files += 1;
                continue;
            }
            let imported_session =
                session_for_import_owner(connection, codex_dir, &session, &owner)?;
            if upsert_codex_imported_session(connection, &imported_session, &owner, owner_pinned)? {
                imported_sessions += 1;
            } else {
                updated_sessions += 1;
            }
            if upsert_codex_state_thread_for_session(
                connection,
                codex_dir,
                &imported_session,
                &owner,
            )? {
                codex_synced_threads += 1;
            } else {
                codex_skipped_threads += 1;
            }
        }

        let project_count = connection
            .query_row("SELECT COUNT(*) FROM local_projects", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        let session_count = connection
            .query_row("SELECT COUNT(*) FROM session_records", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;

        return Ok(CodexLocalSessionImportResult {
            scanned_files,
            imported_sessions,
            updated_sessions,
            skipped_files,
            codex_synced_threads,
            codex_skipped_threads,
            project_count,
            session_count,
            message: format!(
                "已扫描 {} 个 Codex 本地 thread，新增 {} 条，更新 {} 条，跳过 {} 个；已同步到 Codex {} 条，跳过 {} 条。",
                scanned_files,
                imported_sessions,
                updated_sessions,
                skipped_files,
                codex_synced_threads,
                codex_skipped_threads
            ),
        });
    }

    let index = read_codex_session_index(codex_dir)?;
    let mut files = Vec::new();
    collect_codex_session_files(&codex_dir.join("sessions"), &mut files)?;
    files.sort();

    let mut scanned_files = 0_i64;
    let mut imported_sessions = 0_i64;
    let mut updated_sessions = 0_i64;
    let mut skipped_files = 0_i64;
    let mut codex_synced_threads = 0_i64;
    let mut codex_skipped_threads = 0_i64;

    for file in files {
        scanned_files += 1;
        match parse_codex_local_session_file(&file, &index)? {
            Some(session) => {
                if !requested_ids.contains(&session.session_id) {
                    skipped_files += 1;
                    continue;
                }
                let imported_session =
                    session_for_import_owner(connection, codex_dir, &session, &owner)?;
                if upsert_codex_imported_session(
                    connection,
                    &imported_session,
                    &owner,
                    owner_pinned,
                )? {
                    imported_sessions += 1;
                } else {
                    updated_sessions += 1;
                }
                if upsert_codex_state_thread_for_session(
                    connection,
                    codex_dir,
                    &imported_session,
                    &owner,
                )? {
                    codex_synced_threads += 1;
                } else {
                    codex_skipped_threads += 1;
                }
            }
            None => skipped_files += 1,
        }
    }

    let project_count = connection
        .query_row("SELECT COUNT(*) FROM local_projects", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let session_count = connection
        .query_row("SELECT COUNT(*) FROM session_records", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;

    Ok(CodexLocalSessionImportResult {
        scanned_files,
        imported_sessions,
        updated_sessions,
        skipped_files,
        codex_synced_threads,
        codex_skipped_threads,
        project_count,
        session_count,
        message: format!(
            "已扫描 {} 个 Codex 本地 session，新增 {} 条，更新 {} 条，跳过 {} 个；已同步到 Codex {} 条，跳过 {} 条。",
            scanned_files,
            imported_sessions,
            updated_sessions,
            skipped_files,
            codex_synced_threads,
            codex_skipped_threads
        ),
    })
}

fn query_recent_snapshots_for_account(
    connection: &Connection,
    account_id: i64,
    limit: i64,
) -> Result<Vec<UsageSnapshot>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT account_id, sample_time, window_5h_percent, window_7d_percent, risk_level,
                    estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated
             FROM usage_snapshots
             WHERE account_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![account_id, limit], |row| {
            Ok(UsageSnapshot {
                account_id: row.get(0)?,
                sample_time: row.get(1)?,
                window_5h_percent: row.get(2)?,
                window_7d_percent: row.get(3)?,
                risk_level: row.get(4)?,
                estimated_reset_5h_at: row.get(5)?,
                estimated_reset_7d_at: row.get(6)?,
                source_type: row.get(7)?,
                confidence_level: row.get(8)?,
                is_estimated: row.get::<_, i64>(9)? == 1,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn query_recent_notifications_for_account(
    connection: &Connection,
    account_id: i64,
    limit: i64,
) -> Result<Vec<NotificationItem>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, account_id, level, title, message, source_type, action_type, related_handoff_id, created_at
             FROM notifications
             WHERE account_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![account_id, limit], |row| {
            Ok(NotificationItem {
                id: row.get(0)?,
                account_id: row.get(1)?,
                level: row.get(2)?,
                title: row.get(3)?,
                message: row.get(4)?,
                source_type: row.get(5)?,
                action_type: row.get(6)?,
                related_handoff_id: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn query_recent_sessions_for_account(
    connection: &Connection,
    account_id: i64,
    limit: i64,
) -> Result<Vec<SessionRecord>, String> {
    let profile_ref = format!("account:{account_id}");
    let mut stmt = connection
        .prepare(
            "SELECT DISTINCT s.id, s.project_id, p.name, p.workspace_path, s.owner_account_id,
                    s.owner_profile_kind, s.owner_profile_ref, s.record_type, s.title, s.summary,
                    s.raw_content, s.message_count, s.source_record_id, s.created_at, s.updated_at
             FROM session_records s
             JOIN local_projects p ON p.id = s.project_id
             LEFT JOIN session_profile_links l ON l.session_id = s.id
             WHERE s.owner_account_id = ?1
                OR (l.profile_kind = 'official_account' AND l.profile_ref = ?2)
             ORDER BY s.updated_at DESC, s.id DESC
             LIMIT ?3",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![account_id, profile_ref, limit], map_session_record)
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn account_health_timeline(snapshots: &[UsageSnapshot]) -> Vec<TimelineSegment> {
    if snapshots.is_empty() {
        return vec![TimelineSegment {
            state: "unknown".to_string(),
            hours: 1,
            label: "待采样".to_string(),
            tooltip: "还没有真实用量快照，完成采样后会生成健康时间线。".to_string(),
        }];
    }

    snapshots
        .iter()
        .take(10)
        .map(|snapshot| TimelineSegment {
            state: snapshot.risk_level.clone(),
            hours: 1,
            label: snapshot.sample_time.clone(),
            tooltip: format!(
                "5h 剩余 {}，7d 剩余 {}，可信度 {}",
                usage_percent_text(snapshot.window_5h_percent),
                usage_percent_text(snapshot.window_7d_percent),
                snapshot.confidence_level
            ),
        })
        .collect()
}

fn build_account_detail(connection: &Connection, id: i64) -> Result<AccountDetail, String> {
    let account = query_account_by_id(connection, id)?;
    let recent_snapshots = query_recent_snapshots_for_account(connection, id, 10)?;
    let recent_switches = query_recent_switches_for_account(connection, id, 10)?;
    let recent_notifications = query_recent_notifications_for_account(connection, id, 10)?;
    let recent_sessions = query_recent_sessions_for_account(connection, id, 10)?;
    let keychain_readable = if account.is_real_session {
        read_bound_session_snapshot(&account).is_ok()
    } else {
        false
    };
    let bound_snapshot_summary = read_bound_session_snapshot(&account).ok().map(|snapshot| {
        format!(
            "{} · 凭证长度 {}",
            snapshot.session_ref,
            snapshot.credentials_json.len()
        )
    });
    let last_failure_reason = recent_notifications
        .iter()
        .find(|item| item.level == "error" || item.level == "warning")
        .map(|item| format!("{}：{}", item.title, item.message));
    let health_timeline = account_health_timeline(&recent_snapshots);
    let diagnostic_text = format!(
        "账号：{}\n登录邮箱：{}\n官方账号 ID：{}\n状态：{} / {}\nKeychain：{}\n最近采样：{}",
        account.nickname,
        account
            .account_email
            .clone()
            .unwrap_or_else(|| "未读取到邮箱".to_string()),
        account
            .profile_ref
            .clone()
            .unwrap_or_else(|| "--".to_string()),
        account.status,
        account.auth_state,
        if keychain_readable {
            "可读"
        } else {
            "不可读或不存在"
        },
        recent_snapshots
            .first()
            .map(|snapshot| snapshot.sample_time.clone())
            .unwrap_or_else(|| "暂无".to_string())
    );

    Ok(AccountDetail {
        account,
        recent_snapshots,
        recent_switches,
        recent_notifications,
        recent_sessions,
        keychain_readable,
        bound_snapshot_summary,
        last_failure_reason,
        health_timeline,
        diagnostic_text,
    })
}

fn insert_notification(
    connection: &Connection,
    level: &str,
    title: &str,
    message: &str,
    source_type: &str,
) -> Result<(), String> {
    insert_structured_notification(
        connection,
        None,
        level,
        title,
        message,
        source_type,
        source_type,
        None,
    )
}

fn insert_account_notification(
    connection: &Connection,
    account_id: i64,
    level: &str,
    title: &str,
    message: &str,
    source_type: &str,
    action_type: &str,
    related_handoff_id: Option<i64>,
) -> Result<(), String> {
    insert_structured_notification(
        connection,
        Some(account_id),
        level,
        title,
        message,
        source_type,
        action_type,
        related_handoff_id,
    )
}

fn insert_structured_notification(
    connection: &Connection,
    account_id: Option<i64>,
    level: &str,
    title: &str,
    message: &str,
    source_type: &str,
    action_type: &str,
    related_handoff_id: Option<i64>,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO notifications (account_id, level, title, message, source_type, action_type, related_handoff_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account_id,
                level,
                title,
                message,
                source_type,
                action_type,
                related_handoff_id,
                now_text()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn cleanup_preview(connection: &Connection) -> Result<CleanupPreview, String> {
    let old_handoff_count = connection
        .query_row(
            "SELECT COUNT(*) FROM handoff_cards
             WHERE task_title IN ('切换前自动接力', '继续收尾 Day 8')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let old_notification_count = connection
        .query_row(
            "SELECT COUNT(*) FROM notifications
             WHERE account_id IS NULL
               AND (
                    message LIKE '%真实额度暂不可读%'
                    OR title LIKE '%真实额度暂不可读%'
                    OR source_type = 'mock_estimator'
                    OR (source_type = 'settings_event' AND title = '官方扩容优先')
               )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let orphan_handoff_count = connection
        .query_row(
            "SELECT COUNT(*) FROM handoff_cards
             WHERE account_id NOT IN (SELECT id FROM accounts WHERE is_real_session = 1)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    Ok(CleanupPreview {
        old_handoff_count,
        old_notification_count,
        orphan_handoff_count,
    })
}

fn cleanup_historical_debug_data(connection: &Connection) -> Result<CleanupResult, String> {
    let preview = cleanup_preview(connection)?;

    let deleted_old_handoffs = connection
        .execute(
            "DELETE FROM handoff_cards
             WHERE task_title IN ('切换前自动接力', '继续收尾 Day 8')",
            [],
        )
        .map_err(|error| error.to_string())? as i64;
    let deleted_old_notifications = connection
        .execute(
            "DELETE FROM notifications
             WHERE account_id IS NULL
               AND (
                    message LIKE '%真实额度暂不可读%'
                    OR title LIKE '%真实额度暂不可读%'
                    OR source_type = 'mock_estimator'
                    OR (source_type = 'settings_event' AND title = '官方扩容优先')
               )",
            [],
        )
        .map_err(|error| error.to_string())? as i64;
    let deleted_orphan_handoffs = connection
        .execute(
            "DELETE FROM handoff_cards
             WHERE account_id NOT IN (SELECT id FROM accounts WHERE is_real_session = 1)",
            [],
        )
        .map_err(|error| error.to_string())? as i64;

    Ok(CleanupResult {
        old_handoff_count: preview.old_handoff_count,
        old_notification_count: preview.old_notification_count,
        orphan_handoff_count: preview.orphan_handoff_count,
        deleted_total: deleted_old_handoffs + deleted_old_notifications + deleted_orphan_handoffs,
    })
}

fn chart_source_label(source_type: &str) -> &'static str {
    match source_type {
        "real_usage" => "真实采样",
        "unknown" => "未知",
        _ => "混合来源",
    }
}

const CHART_BUCKET_MINUTES: u32 = 15;
const MAX_CHART_POINTS: usize = 6;

fn chart_bucket_start_text(sample_time: &str) -> Option<String> {
    let sample_at = parse_local_datetime_text(sample_time)?;
    let bucket_minute = (sample_at.minute() / CHART_BUCKET_MINUTES) * CHART_BUCKET_MINUTES;
    sample_at
        .with_minute(bucket_minute)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn sampling_summary(kind: &str, message: &str, source_type: &str) -> SamplingSummary {
    SamplingSummary {
        kind: kind.to_string(),
        message: message.to_string(),
        source_type: source_type.to_string(),
    }
}

fn default_usage_display() -> UsageDisplayState {
    UsageDisplayState {
        status: "unknown".to_string(),
        source_type: "unknown".to_string(),
        confidence_label: "未知".to_string(),
        summary: "当前还没有绑定真实账号".to_string(),
        helper_text: "请先完成官方登录并绑定当前账号。".to_string(),
        chart_helper_text: "当前暂无真实用量历史，请先登录并绑定真实账号。".to_string(),
    }
}

fn has_auth_issue(account: &Account) -> bool {
    account.auth_state != "valid" || account.status == "auth_invalid"
}

fn build_real_unknown_usage_display(account: &Account) -> UsageDisplayState {
    if has_auth_issue(account) {
        return UsageDisplayState {
            status: "unknown".to_string(),
            source_type: "unknown".to_string(),
            confidence_label: "待校验".to_string(),
            summary: "当前真实账号登录态异常，暂时无法展示真实用量".to_string(),
            helper_text: "请先重新校验或重新绑定当前账号；在登录态恢复前只显示未知状态。"
                .to_string(),
            chart_helper_text:
                "真实账号图表只展示真实采样历史；当前登录态异常，因此会明确保持未知状态。"
                    .to_string(),
        };
    }

    UsageDisplayState {
        status: "unknown".to_string(),
        source_type: "unknown".to_string(),
        confidence_label: "未知".to_string(),
        summary: "真实登录态已验证，但当前仍没有可展示的真实用量快照".to_string(),
        helper_text: "当前仅能确认官方登录态有效；还没有面向该登录态的稳定真实额度读取链路。"
            .to_string(),
        chart_helper_text: "真实账号当前只展示可验证来源的数据；拿不到真实快照时会明确显示未知。"
            .to_string(),
    }
}

fn build_usage_display_state(
    active_account: Option<&Account>,
    latest_snapshot: Option<&UsageSnapshot>,
) -> UsageDisplayState {
    match active_account {
        None => default_usage_display(),
        Some(account) => {
            if let Some(snapshot) = latest_snapshot {
                return UsageDisplayState {
                    status: "ready".to_string(),
                    source_type: snapshot.source_type.clone(),
                    confidence_label: snapshot.confidence_level.clone(),
                    summary: format!(
                        "5h 剩余 {} · 7d 剩余 {} · 5h 恢复 {} · 7d 恢复 {} · {}",
                        usage_percent_text(snapshot.window_5h_percent),
                        usage_percent_text(snapshot.window_7d_percent),
                        snapshot.estimated_reset_5h_at.as_deref().unwrap_or("未知"),
                        snapshot.estimated_reset_7d_at.as_deref().unwrap_or("未知"),
                        chart_source_label(&snapshot.source_type)
                    ),
                    helper_text: chart_source_label(&snapshot.source_type).to_string(),
                    chart_helper_text: match snapshot.source_type.as_str() {
                        "real_usage" => "当前图表展示真实采样历史。".to_string(),
                        _ => "当前图表包含非标准来源，请谨慎判断。".to_string(),
                    },
                };
            }

            if account.is_real_session {
                build_real_unknown_usage_display(account)
            } else {
                default_usage_display()
            }
        }
    }
}

fn latest_sampling_summary(
    active_account: Option<&Account>,
    latest_snapshot: Option<&UsageSnapshot>,
) -> SamplingSummary {
    if let Some(snapshot) = latest_snapshot {
        return sampling_summary("real_updated", "真实采样已更新", &snapshot.source_type);
    }

    if let Some(account) = active_account {
        if account.is_real_session {
            return if has_auth_issue(account) {
                sampling_summary(
                    "real_unknown",
                    "已完成采样，但当前真实账号登录态异常，未生成真实用量快照",
                    "unknown",
                )
            } else {
                sampling_summary(
                    "real_unknown",
                    "已完成登录态校验，但当前仍没有可读的真实用量快照",
                    "unknown",
                )
            };
        }
    }

    sampling_summary(
        "idle",
        "当前没有可采样的真实账号，请先完成官方登录并绑定账号",
        "unknown",
    )
}

fn current_codex_login(accounts: &[Account]) -> Option<CurrentCodexLogin> {
    let status = read_codex_auth_status_cached().ok()?;
    if !status.logged_in {
        return Some(CurrentCodexLogin {
            logged_in: false,
            email: status.account_email,
            account_id: status.account_id,
            is_bound: false,
        });
    }

    let is_bound = accounts.iter().any(|account| {
        status
            .account_email
            .as_deref()
            .is_some_and(|email| account.account_email.as_deref() == Some(email))
            || status
                .account_id
                .as_deref()
                .is_some_and(|account_id| account.profile_ref.as_deref() == Some(account_id))
    });

    Some(CurrentCodexLogin {
        logged_in: true,
        email: status.account_email,
        account_id: status.account_id,
        is_bound,
    })
}

fn normalize_account_plan_label(plan_type: &str) -> Option<String> {
    match plan_type.trim().to_ascii_lowercase().as_str() {
        "plus" => Some("plus".to_string()),
        "pro" => Some("pro".to_string()),
        "team" | "business" | "enterprise" => Some("team".to_string()),
        _ => None,
    }
}

fn extract_account_plan_label(raw_meta_json: &str) -> Option<String> {
    let payload = serde_json::from_str::<Value>(raw_meta_json).ok()?;
    let plan_type = payload
        .get("payload")
        .and_then(|item| item.get("plan_type"))
        .and_then(|item| item.as_str())?;
    normalize_account_plan_label(plan_type)
}

fn query_latest_account_plan_label(
    connection: &Connection,
    account_id: i64,
) -> Result<Option<String>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT raw_meta_json
             FROM usage_snapshots
             WHERE account_id = ?1 AND source_type = 'real_usage'
             ORDER BY id DESC
             LIMIT 1",
        )
        .map_err(|error| error.to_string())?;

    let mut rows = stmt
        .query([account_id])
        .map_err(|error| error.to_string())?;

    if let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let raw_meta_json: String = row.get(0).map_err(|error| error.to_string())?;
        return Ok(extract_account_plan_label(&raw_meta_json));
    }

    Ok(None)
}

fn build_chart_points(connection: &Connection) -> Result<Vec<ChartPoint>, String> {
    let accounts = query_accounts(connection)?;
    let tracked_accounts = accounts
        .into_iter()
        .filter(|account| account.is_real_session)
        .take(4)
        .collect::<Vec<_>>();

    let mut stmt = connection
        .prepare(
            "SELECT account_id, sample_time, window_5h_percent, source_type
             FROM usage_snapshots
             WHERE source_type = 'real_usage'
             ORDER BY sample_time DESC, id DESC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let mut grouped = Vec::<(String, Vec<ChartSeriesValue>, String)>::new();
    for row in rows {
        let (account_id, sample_time, value, source_type) =
            row.map_err(|error| error.to_string())?;
        let Some(account) = tracked_accounts
            .iter()
            .find(|account| account.id == account_id)
        else {
            continue;
        };
        let bucket_time = chart_bucket_start_text(&sample_time).unwrap_or(sample_time.clone());

        if let Some((time, series, _)) =
            grouped.iter_mut().find(|(time, _, _)| *time == bucket_time)
        {
            if !series.iter().any(|item| item.account_id == account_id) {
                let _ = time;
                series.push(ChartSeriesValue {
                    account_id,
                    account_name: account.nickname.clone(),
                    value,
                });
            }
            continue;
        }

        grouped.push((
            bucket_time,
            vec![ChartSeriesValue {
                account_id,
                account_name: account.nickname.clone(),
                value,
            }],
            source_type,
        ));

        if grouped.len() >= MAX_CHART_POINTS {
            break;
        }
    }

    grouped.reverse();

    let mut points = grouped
        .into_iter()
        .map(|(sample_time, mut series, source_type)| {
            series.sort_by_key(|item| {
                tracked_accounts
                    .iter()
                    .position(|account| account.id == item.account_id)
                    .unwrap_or(usize::MAX)
            });
            let event_label = if series.iter().any(|item| item.value <= 15) {
                Some("预警".to_string())
            } else {
                None
            };
            ChartPoint {
                label: sample_time.chars().skip(11).take(5).collect::<String>(),
                series,
                event_label,
                source_label: chart_source_label(&source_type).to_string(),
            }
        })
        .collect::<Vec<_>>();

    if points.is_empty() {
        points.push(ChartPoint {
            label: "现在".into(),
            series: Vec::new(),
            event_label: None,
            source_label: "未知".into(),
        });
    }

    let switch_logs = query_switch_logs(connection)?;
    if let Some(last_success) = switch_logs.iter().find(|log| log.result == "success") {
        if let Some(last_point) = points.last_mut() {
            last_point.event_label = Some(format!(
                "切换@{}",
                last_success
                    .created_at
                    .chars()
                    .skip(11)
                    .take(5)
                    .collect::<String>()
            ));
        }
    }

    Ok(points)
}

fn latest_display_snapshot(
    connection: &Connection,
    account: &Account,
) -> Result<Option<UsageSnapshot>, String> {
    query_latest_real_usage_snapshot(connection, account.id)
}

#[derive(Debug, Clone)]
struct RecommendationCandidate {
    account_id: i64,
    max_percent: i64,
    total_percent: i64,
    earliest_reset_at: Option<chrono::DateTime<Local>>,
    reason: String,
}

fn best_reset_time(snapshot: &UsageSnapshot) -> Option<chrono::DateTime<Local>> {
    [
        snapshot.estimated_reset_5h_at.as_deref(),
        snapshot.estimated_reset_7d_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(parse_local_datetime_text)
    .min()
}

fn compare_optional_reset(
    left: Option<chrono::DateTime<Local>>,
    right: Option<chrono::DateTime<Local>>,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_candidates(left: &RecommendationCandidate, right: &RecommendationCandidate) -> Ordering {
    right
        .max_percent
        .cmp(&left.max_percent)
        .then(right.total_percent.cmp(&left.total_percent))
        .then(compare_optional_reset(
            left.earliest_reset_at,
            right.earliest_reset_at,
        ))
        .then(left.account_id.cmp(&right.account_id))
}

fn recommended_switch_candidate(
    connection: &Connection,
    accounts: &[Account],
) -> Result<Option<RecommendationCandidate>, String> {
    let mut best: Option<RecommendationCandidate> = None;

    for account in accounts.iter().filter(|account| !account.is_active) {
        if has_auth_issue(account) || is_switchable(account).is_err() {
            continue;
        }

        let Some(snapshot) = latest_display_snapshot(connection, account)? else {
            continue;
        };

        let known_windows = [
            ("5h", snapshot.window_5h_percent),
            ("7d", snapshot.window_7d_percent),
        ]
        .into_iter()
        .filter(|(_, percent)| *percent >= 0)
        .collect::<Vec<_>>();
        let Some((risk_window, max_percent)) = known_windows
            .iter()
            .min_by_key(|(_, percent)| *percent)
            .copied()
        else {
            continue;
        };
        let total_percent = known_windows.iter().map(|(_, percent)| percent).sum();
        let reason = format!(
            "{} 当前 5h 剩余 {}，7d 剩余 {}，优先风险来自 {} 窗口。",
            account.nickname,
            usage_percent_text(snapshot.window_5h_percent),
            usage_percent_text(snapshot.window_7d_percent),
            risk_window
        );

        let candidate = RecommendationCandidate {
            account_id: account.id,
            max_percent,
            total_percent,
            earliest_reset_at: best_reset_time(&snapshot),
            reason,
        };

        if best
            .as_ref()
            .is_none_or(|current| compare_candidates(&candidate, current).is_lt())
        {
            best = Some(candidate);
        }
    }

    Ok(best)
}

fn timeline_segment(state: &str, hours: i64, label: &str, tooltip: String) -> TimelineSegment {
    TimelineSegment {
        state: state.to_string(),
        hours,
        label: label.to_string(),
        tooltip,
    }
}

fn build_timeline(connection: &Connection, accounts: &[Account]) -> Vec<TimelineLane> {
    let recommended_account_id = recommended_switch_candidate(connection, accounts)
        .ok()
        .flatten()
        .map(|candidate| candidate.account_id);

    accounts
        .iter()
        .map(|account| {
            let latest = latest_display_snapshot(connection, account).ok().flatten();
            let confidence = if account.is_real_session && latest.is_none() {
                if has_auth_issue(account) {
                    "真实会话 · 待修复"
                } else {
                    "真实会话 · 待采样"
                }
            } else {
                "精确 · 12h"
            };

            if has_auth_issue(account) {
                return TimelineLane {
                    account_id: account.id,
                    account_name: account.nickname.clone(),
                    confidence: confidence.to_string(),
                    next_action: "先重绑并重新采样，再参与推荐。".to_string(),
                    segments: vec![
                        timeline_segment(
                            "auth_invalid",
                            4,
                            "需修复",
                            "登录态异常，当前不参与切换。".to_string(),
                        ),
                        timeline_segment(
                            "unknown",
                            4,
                            "待重绑",
                            "完成重绑后再校验真实额度。".to_string(),
                        ),
                        timeline_segment(
                            "unknown",
                            4,
                            "待采样",
                            "恢复后重新生成推荐。".to_string(),
                        ),
                    ],
                };
            }

            let Some(snapshot) = latest else {
                return TimelineLane {
                    account_id: account.id,
                    account_name: account.nickname.clone(),
                    confidence: confidence.to_string(),
                    next_action: "先执行真实采样，确认 5h / 7d 窗口。".to_string(),
                    segments: vec![
                        timeline_segment("unknown", 4, "待采样", "缺少真实用量快照。".to_string()),
                        timeline_segment(
                            "unknown",
                            4,
                            "待确认",
                            "采样后才能判断是否可切换。".to_string(),
                        ),
                        timeline_segment(
                            "unknown",
                            4,
                            "待推荐",
                            "当前不生成未来预测。".to_string(),
                        ),
                    ],
                };
            };

            let (dominant_label, dominant_percent) = [
                ("5h", snapshot.window_5h_percent),
                ("7d", snapshot.window_7d_percent),
            ]
            .into_iter()
            .filter(|(_, percent)| *percent >= 0)
            .min_by_key(|(_, percent)| *percent)
            .unwrap_or(("未知", 100));
            let window_5h_text = usage_percent_text(snapshot.window_5h_percent);
            let window_7d_text = usage_percent_text(snapshot.window_7d_percent);
            let reset_5h = snapshot.estimated_reset_5h_at.as_deref().unwrap_or("未知");
            let reset_7d = snapshot.estimated_reset_7d_at.as_deref().unwrap_or("未知");
            let recommended_now = recommended_account_id == Some(account.id);

            let (next_action, segments) = if dominant_percent <= 0 {
                (
                    format!("等待 {} 窗口恢复后再参与切换。", dominant_label),
                    vec![
                        timeline_segment(
                            "exhausted",
                            4,
                            "等待恢复",
                            format!(
                                "5h 剩余 {}，7d 剩余 {}；5h 恢复 {}；7d 恢复 {}",
                                window_5h_text, window_7d_text, reset_5h, reset_7d
                            ),
                        ),
                        timeline_segment(
                            "exhausted",
                            4,
                            "暂不可切",
                            format!("主风险来自 {} 窗口。", dominant_label),
                        ),
                        timeline_segment(
                            "warning",
                            4,
                            "恢复观察",
                            "恢复后建议先观察再参与切换。".to_string(),
                        ),
                    ],
                )
            } else if dominant_percent <= 15 {
                (
                    format!("处于预警区，优先观察 {} 窗口并准备接力。", dominant_label),
                    vec![
                        timeline_segment(
                            "warning",
                            4,
                            "预警观察",
                            format!(
                                "5h 剩余 {}，7d 剩余 {}；主风险来自 {} 窗口。",
                                window_5h_text, window_7d_text, dominant_label
                            ),
                        ),
                        timeline_segment(
                            "warning",
                            4,
                            "准备切换",
                            format!(
                                "建议在恢复前保存必要上下文；5h 恢复 {}；7d 恢复 {}",
                                reset_5h, reset_7d
                            ),
                        ),
                        timeline_segment(
                            "healthy",
                            4,
                            "恢复后复查",
                            "恢复后可重新参与最优候选排序。".to_string(),
                        ),
                    ],
                )
            } else {
                (
                    if recommended_now {
                        "现在可切换，建议作为当前最优接力账号。".to_string()
                    } else {
                        format!("当前可用，继续观察 {} 窗口变化。", dominant_label)
                    },
                    vec![
                        timeline_segment(
                            "healthy",
                            4,
                            if recommended_now {
                                "现在可切"
                            } else {
                                "当前可用"
                            },
                            format!(
                                "5h 剩余 {}，7d 剩余 {}；5h 恢复 {}；7d 恢复 {}",
                                window_5h_text, window_7d_text, reset_5h, reset_7d
                            ),
                        ),
                        timeline_segment(
                            if dominant_percent <= 30 {
                                "warning"
                            } else {
                                "healthy"
                            },
                            4,
                            if dominant_percent <= 30 {
                                "预警观察"
                            } else {
                                "继续可用"
                            },
                            format!("主风险来自 {} 窗口。", dominant_label),
                        ),
                        timeline_segment(
                            if dominant_percent <= 30 {
                                "warning"
                            } else {
                                "healthy"
                            },
                            4,
                            "下一次判断",
                            "建议结合下一轮采样决定是否切换。".to_string(),
                        ),
                    ],
                )
            };

            TimelineLane {
                account_id: account.id,
                account_name: account.nickname.clone(),
                confidence: confidence.to_string(),
                next_action,
                segments,
            }
        })
        .collect()
}

fn build_recommendations(
    connection: &Connection,
    accounts: &[Account],
    active_account: Option<&Account>,
    latest_snapshot: Option<&UsageSnapshot>,
    settings: &AppSettings,
) -> Result<(Vec<String>, Option<i64>, Option<String>), String> {
    let mut recommendations = Vec::new();
    let candidate = recommended_switch_candidate(connection, accounts)?;
    let recommended_account_id = candidate.as_ref().map(|item| item.account_id);
    let recommended_reason = candidate.as_ref().map(|item| item.reason.clone());

    if settings.prefer_official_upgrade {
        recommendations.push("优先提示官方扩容方案，切换账号作为备选动作。".to_string());
    }

    if active_account.is_none() {
        recommendations
            .push("请先启动官方登录流程并绑定当前账号，之后再进行采样和切换。".to_string());
    }

    if let Some(active) = active_account {
        if has_auth_issue(active) {
            recommendations.push(format!(
                "{} 当前登录态异常，建议先重新绑定或修复授权。",
                active.nickname
            ));
        } else if active.is_real_session && latest_snapshot.is_none() {
            recommendations.push("当前活跃账号已完成真实绑定，但仍没有稳定真实额度快照；当前只基于登录态和切换能力给出保守建议。".to_string());
            if let Some(reason) = &recommended_reason {
                recommendations.push(format!("如需切换，优先候选：{}", reason));
            } else {
                recommendations.push(
                    "当前没有可直接切换的备用账号，建议先重新绑定可用账号或优先走官方扩容。"
                        .to_string(),
                );
            }
        } else if let Some(snapshot) = latest_snapshot {
            let lowest_remaining =
                lowest_known_remaining(snapshot.window_5h_percent, snapshot.window_7d_percent)
                    .unwrap_or(100);
            let high_remaining_warning = (100 - settings.warn_threshold_high).max(0);
            let mid_remaining_warning = (100 - settings.warn_threshold_mid).max(0);
            if lowest_remaining <= 0 {
                if let Some(candidate) = accounts
                    .iter()
                    .find(|account| Some(account.id) == recommended_account_id)
                {
                    recommendations.push(format!(
                        "推荐立即切换到 {}，当前活跃账号已有额度窗口剩余为 0%。",
                        candidate.nickname
                    ));
                } else {
                    recommendations.push(
                        "当前账号额度窗口剩余为 0%，且没有健康备用账号，建议先走官方扩容或等待恢复。"
                            .to_string(),
                    );
                }
            } else if lowest_remaining <= high_remaining_warning {
                if let Some(candidate) = accounts
                    .iter()
                    .find(|account| Some(account.id) == recommended_account_id)
                {
                    recommendations.push(format!(
                        "推荐立即切换到 {}，当前活跃账号已接近耗尽。",
                        candidate.nickname
                    ));
                } else {
                    recommendations
                        .push("当前没有健康备用账号，建议先走官方扩容或修复授权。".to_string());
                }
            } else if lowest_remaining <= mid_remaining_warning {
                if let Some(candidate) = accounts
                    .iter()
                    .find(|account| Some(account.id) == recommended_account_id)
                {
                    recommendations.push(format!(
                        "建议优先准备切到 {}，避免进入耗尽区。",
                        candidate.nickname
                    ));
                } else {
                    recommendations.push(
                        "当前进入预警区，但没有更优备用账号，建议优先检查官方扩容。".to_string(),
                    );
                }
            } else {
                recommendations.push("当前活跃账号仍可继续使用，不建议频繁切换。".to_string());
            }

            if let Some(reason) = &recommended_reason {
                recommendations.push(format!("推荐依据：{}", reason));
            }
        }

        recommendations.push(format!(
            "当前主账号为 {}，建议继续观察最近校验时间与恢复状态。",
            active.nickname
        ));
    }

    recommendations.truncate(4);
    Ok((recommendations, recommended_account_id, recommended_reason))
}

#[cfg(test)]
#[allow(dead_code)]
fn sample_accounts_with_notifications(
    connection: &Connection,
    accounts: &[Account],
    notification_limit: i32,
) -> Result<(), String> {
    let now = Local::now();
    let mut created_notifications = 0;

    let failures = run_resilient_sampling_cycle(accounts, |account| {
        let real_updated = sample_real_account_usage(connection, account, now)?;
        let refreshed_account = query_account_by_id(connection, account.id)?;

        if refreshed_account.auth_state == "mismatch" && created_notifications < notification_limit
        {
            insert_account_notification(
                connection,
                refreshed_account.id,
                "warning",
                &format!("{} 登录态异常", refreshed_account.nickname),
                "当前真实账号与官方登录态不一致；本轮仅更新校验状态，未生成真实用量快照。",
                "real_verification",
                "sample_mismatch",
                None,
            )?;
            created_notifications += 1;
        } else if !real_updated && created_notifications < notification_limit {
            insert_account_notification(
                connection,
                refreshed_account.id,
                "info",
                &format!("{} 真实额度暂不可读", refreshed_account.nickname),
                "当前已完成真实登录态校验，但还没有稳定真实额度读取链路；本轮保持 unknown 展示，不会写入任何非真实数据。",
                "real_verification",
                "sample_unavailable",
                None,
            )?;
            created_notifications += 1;
        }

        Ok(real_updated)
    });

    if !failures.is_empty() {
        return Err(summarize_sampling_failures(&failures));
    }

    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
fn generate_usage_snapshots(connection: &Connection) -> Result<(), String> {
    let accounts = automatic_sampling_accounts(&query_accounts(connection)?, Local::now());
    sample_accounts_with_notifications(connection, &accounts, 3)
}

#[cfg(test)]
#[allow(dead_code)]
fn generate_active_usage_snapshot(connection: &Connection) -> Result<(), String> {
    let accounts = query_accounts(connection)?;
    let target_accounts = accounts
        .iter()
        .find(|account| account.is_active && account.is_real_session)
        .cloned()
        .or_else(|| {
            accounts
                .iter()
                .find(|account| account.is_default && account.is_real_session)
                .cloned()
        })
        .or_else(|| {
            accounts
                .iter()
                .find(|account| account.is_real_session)
                .cloned()
        })
        .into_iter()
        .collect::<Vec<_>>();

    if target_accounts.is_empty() {
        return Ok(());
    }

    sample_accounts_with_notifications(connection, &target_accounts, 1)
}

#[cfg(test)]
fn active_sampling_accounts(accounts: &[Account]) -> Vec<Account> {
    accounts
        .iter()
        .find(|account| account.is_active && account.is_real_session)
        .cloned()
        .or_else(|| {
            accounts
                .iter()
                .find(|account| account.is_default && account.is_real_session)
                .cloned()
        })
        .or_else(|| {
            accounts
                .iter()
                .find(|account| account.is_real_session)
                .cloned()
        })
        .into_iter()
        .collect()
}

fn snapshot_reset_due(snapshot: &UsageSnapshot, now: chrono::DateTime<Local>) -> bool {
    let sample_time = parse_local_datetime_text(&snapshot.sample_time);
    [
        snapshot.estimated_reset_5h_at.as_deref(),
        snapshot.estimated_reset_7d_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(parse_local_datetime_text)
    .any(|reset_at| {
        if let Some(sample_time) = sample_time {
            sample_time < reset_at && reset_at <= now
        } else {
            reset_at <= now
        }
    })
}

fn should_auto_sample_account(account: &Account, now: chrono::DateTime<Local>) -> bool {
    if !account.is_real_session {
        return false;
    }

    if account.is_active {
        return true;
    }

    account
        .latest_snapshot
        .as_ref()
        .map(|snapshot| snapshot_reset_due(snapshot, now))
        .unwrap_or(true)
}

fn automatic_sampling_accounts(accounts: &[Account], now: chrono::DateTime<Local>) -> Vec<Account> {
    accounts
        .iter()
        .filter(|account| should_auto_sample_account(account, now))
        .cloned()
        .collect()
}

fn sample_accounts_without_long_db_lock(
    state: &AppState,
    accounts: &[Account],
    notification_limit: i32,
) -> Result<(), String> {
    let mut created_notifications = 0;
    let mut failures = Vec::new();

    for account in accounts {
        let outcome = collect_real_account_sampling_outcome(account);
        let now = Local::now();
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;

        match outcome {
            Ok(outcome) => {
                apply_background_sampling_outcome(
                    &connection,
                    account,
                    now,
                    outcome,
                    &mut created_notifications,
                    notification_limit,
                )?;
            }
            Err(error) => failures.push(format!("{}：{}", account.nickname, error)),
        }
    }

    if !failures.is_empty() {
        return Err(summarize_sampling_failures(&failures));
    }

    Ok(())
}

fn generate_usage_snapshots_for_state(state: &AppState) -> Result<(), String> {
    let accounts = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        automatic_sampling_accounts(&query_accounts(&connection)?, Local::now())
    };

    sample_accounts_without_long_db_lock(state, &accounts, 3)
}

#[cfg(test)]
#[allow(dead_code)]
fn generate_active_usage_snapshot_for_state(state: &AppState) -> Result<(), String> {
    let target_accounts = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        active_sampling_accounts(&query_accounts(&connection)?)
    };

    sample_accounts_without_long_db_lock(state, &target_accounts, 1)
}

fn dashboard_overview(connection: &Connection) -> Result<DashboardOverview, String> {
    let accounts = query_accounts(connection)?;
    let settings = query_settings(connection)?;
    let current_login = current_codex_login(&accounts);
    let active_account = accounts.iter().find(|account| account.is_active).cloned();
    let latest_snapshot = if let Some(account) = &active_account {
        latest_display_snapshot(connection, account)?
    } else {
        None
    };
    let usage_display =
        build_usage_display_state(active_account.as_ref(), latest_snapshot.as_ref());
    let latest_sampling =
        latest_sampling_summary(active_account.as_ref(), latest_snapshot.as_ref());

    let (recommendations, recommended_account_id, recommended_reason) = build_recommendations(
        connection,
        &accounts,
        active_account.as_ref(),
        latest_snapshot.as_ref(),
        &settings,
    )?;

    Ok(DashboardOverview {
        active_account,
        accounts: accounts.clone(),
        current_login,
        latest_snapshot,
        usage_display,
        latest_sampling,
        chart_points: build_chart_points(connection)?,
        timeline: build_timeline(connection, &accounts),
        recommendations,
        recommended_account_id,
        recommended_reason,
        switch_logs: query_switch_logs(connection)?,
        settings,
    })
}

fn account_diagnostic_advice(account: &Account, keychain_readable: bool) -> String {
    if account.auth_state == "mismatch" {
        return "官方登录态与绑定账号不一致，建议重新登录并重绑。".to_string();
    }
    if account.auth_state == "expired" || account.status == "auth_invalid" {
        return "登录态已失效，建议重新登录并重绑。".to_string();
    }
    if !keychain_readable {
        return "Keychain 凭证不可读，建议重新绑定该账号。".to_string();
    }
    if account.latest_snapshot.is_none() {
        return "账号已绑定，但还没有真实采样快照，建议立即采样。".to_string();
    }
    "账号状态正常，可参与真实切换与采样。".to_string()
}

fn build_release_diagnostic(connection: &Connection) -> Result<ReleaseDiagnostic, String> {
    sync_real_account_storage(connection)?;
    let accounts = query_accounts(connection)?;
    let active_account = accounts.iter().find(|account| account.is_active).cloned();
    let latest_snapshot = if let Some(account) = &active_account {
        latest_display_snapshot(connection, account)?
    } else {
        None
    };
    let latest_sampling =
        latest_sampling_summary(active_account.as_ref(), latest_snapshot.as_ref());
    let switch_logs = query_switch_logs(connection)?;
    let cli_path = resolve_codex_cli_path().ok();

    let account_diagnostics = accounts
        .iter()
        .map(|account| {
            let keychain_readable = read_bound_session_snapshot(account).is_ok();
            let latest_switch_at = latest_switch_for_account(connection, account.id)?;
            Ok(AccountDiagnostic {
                account_id: account.id,
                nickname: account.nickname.clone(),
                email: account.account_email.clone(),
                profile_ref: account.profile_ref.clone(),
                status: account.status.clone(),
                auth_state: account.auth_state.clone(),
                keychain_readable,
                latest_sample_at: account
                    .latest_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.sample_time.clone()),
                latest_switch_at,
                advice: account_diagnostic_advice(account, keychain_readable),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(ReleaseDiagnostic {
        generated_at: now_text(),
        codex_cli_available: cli_path.is_some(),
        codex_cli_path: cli_path.map(|path| path.display().to_string()),
        current_login: current_codex_login(&accounts),
        database_ok: true,
        account_count: accounts.len() as i64,
        latest_sampling,
        latest_switch: switch_logs.into_iter().next(),
        accounts: account_diagnostics,
    })
}

fn ensure_one_active(connection: &Connection) -> Result<(), String> {
    let active_key_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM credential_profiles
             WHERE profile_kind = 'third_party_key' AND is_active = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_key_count > 0 {
        return Ok(());
    }
    if read_codex_auth_status_cached()
        .ok()
        .and_then(|status| runtime_openai_api_key(&status))
        .is_some()
    {
        return Ok(());
    }

    let active_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_active = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    if active_count == 0 {
        connection
            .execute(
                "UPDATE accounts SET is_active = 1 WHERE id = (SELECT id FROM accounts ORDER BY is_default DESC, id ASC LIMIT 1)",
                [],
            )
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn query_account_by_id(connection: &Connection, id: i64) -> Result<Account, String> {
    migrate_legacy_session_refs(connection)?;

    let mut account = connection
        .query_row(
            "SELECT id, provider, nickname, status, is_active, is_default, auth_state, last_check_time, estimated_reset_time,
                    account_key, binding_kind, session_ref, profile_ref, account_email, last_verified_at, is_real_session
             FROM accounts WHERE id = ?1",
            [id],
            |row| {
                Ok(Account {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    nickname: row.get(2)?,
                    status: row.get(3)?,
                    is_active: row.get::<_, i64>(4)? == 1,
                    is_default: row.get::<_, i64>(5)? == 1,
                    auth_state: row.get(6)?,
                    last_check_time: row.get(7)?,
                    estimated_reset_time: row.get(8)?,
                    account_key: row.get(9)?,
                    binding_kind: row.get(10)?,
                    session_ref: row.get(11)?,
                    profile_ref: row.get(12)?,
                    account_email: row.get(13)?,
                    last_verified_at: row.get(14)?,
                    is_real_session: row.get::<_, i64>(15)? == 1,
                    plan_label: None,
                    latest_snapshot: None,
                })
            },
        )
        .map_err(|_| "目标账号不存在".to_string())?;

    account.latest_snapshot = query_latest_real_usage_snapshot(connection, id)?;
    account.plan_label = query_latest_account_plan_label(connection, id)?;
    if let Some(snapshot) = &account.latest_snapshot {
        if account.auth_state == "valid" && snapshot.source_type == "real_usage" {
            account.status = effective_account_status_from_snapshot(snapshot);
        }
    }
    Ok(account)
}

fn validate_post_switch(
    connection: &Connection,
    target_account_id: i64,
    live_snapshot: Option<&SessionSnapshot>,
) -> Result<(), String> {
    let target = query_account_by_id(connection, target_account_id)?;
    is_switchable(&target)?;

    if target.is_real_session {
        let live_snapshot = live_snapshot
            .ok_or_else(|| "缺少当前官方会话快照，无法完成真实切换校验。".to_string())?;
        if !bound_snapshot_matches_account(&target, live_snapshot) {
            return Err("切换后官方会话与目标账号不一致，已回滚到原账号".to_string());
        }
        return Ok(());
    }

    if let Some(snapshot) = query_latest_snapshot(connection, target_account_id)? {
        if snapshot.risk_level == "exhausted" {
            return Err("切换后检测到目标账号已耗尽，已回滚到原账号".to_string());
        }
        if snapshot.risk_level == "error" || snapshot.risk_level == "auth_invalid" {
            return Err("切换后健康校验失败，已回滚到原账号".to_string());
        }
    }

    Ok(())
}

#[tauri::command]
fn bind_current_codex_account(
    input: BindCurrentCodexAccountInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Account, String> {
    let nickname = input.nickname.trim();
    if nickname.is_empty() {
        return Err("请输入账号昵称".to_string());
    }

    let verified = verify_real_codex_session()?;
    let live_snapshot = live_session_snapshot(&verified)?;
    let config_dir = codex_config_dir()?;
    write_official_account_runtime_files(&config_dir, &live_snapshot.credentials_json)?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let now = now_text();
    let identity_for_key = verified
        .account_id
        .as_deref()
        .or(verified.account_email.as_deref())
        .unwrap_or(nickname);
    let account_key = format!("codex-{}", normalize_account_key(identity_for_key));
    let stored_session_ref = store_account_secret(&account_key, &live_snapshot.credentials_json)?;
    let profile_ref = verified.account_id.clone();
    let account_email = verified.account_email.clone();

    let existing_id = connection
        .query_row(
            "SELECT id FROM accounts
             WHERE account_key = ?1
                OR (?2 != '' AND account_email = ?2)
                OR (?3 != '' AND profile_ref = ?3)
             LIMIT 1",
            params![
                account_key,
                account_email.clone().unwrap_or_default(),
                profile_ref.clone().unwrap_or_default()
            ],
            |row| row.get::<_, i64>(0),
        )
        .ok();

    let existing_id = if existing_id.is_some() {
        existing_id
    } else {
        connection
            .query_row(
                "SELECT id FROM accounts
                 WHERE ?1 != ''
                   AND nickname = ?1
                   AND binding_kind = 'codex_cli'
                   AND (
                        profile_ref IS NULL OR profile_ref = ''
                        OR account_email IS NULL OR account_email = ''
                   )
                 LIMIT 1",
                params![nickname],
                |row| row.get::<_, i64>(0),
            )
            .ok()
    };

    if let Some(id) = existing_id {
        connection
            .execute(
                "UPDATE accounts
                 SET provider = 'Codex', nickname = ?1, status = 'healthy', auth_state = 'valid',
                     account_key = ?2, binding_kind = 'codex_cli', session_ref = ?3, profile_ref = ?4,
                     account_email = ?5, last_verified_at = ?6, last_check_time = ?6, is_real_session = 1, updated_at = ?6
                 WHERE id = ?7",
                params![nickname, account_key, stored_session_ref, profile_ref, account_email, now, id],
            )
            .map_err(|error| error.to_string())?;

        let updated = query_account_by_id(&connection, id)?;
        insert_account_notification(
            &connection,
            updated.id,
            "success",
            "账号已重新绑定",
            &format!("{} 已绑定到当前官方登录态。", updated.nickname),
            "real_binding",
            "account_rebound",
            None,
        )?;
        sync_account_credential_profiles(&connection)?;
        return Ok(updated);
    }

    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;

    connection
        .execute(
            "INSERT INTO accounts (provider, nickname, status, is_active, is_default, auth_state, last_check_time, estimated_reset_time, account_key, binding_kind, session_ref, profile_ref, account_email, last_verified_at, is_real_session, created_at, updated_at)
             VALUES ('Codex', ?1, 'healthy', ?2, ?2, 'valid', ?3, NULL, ?4, 'codex_cli', ?5, ?6, ?7, ?3, 1, ?3, ?3)",
            params![nickname, if count == 0 { 1 } else { 0 }, now, account_key, stored_session_ref, profile_ref, account_email],
        )
        .map_err(|error| error.to_string())?;

    let id = connection.last_insert_rowid();
    let account = query_account_by_id(&connection, id)?;
    sync_account_credential_profiles(&connection)?;
    insert_account_notification(
        &connection,
        account.id,
        "success",
        "账号绑定成功",
        &format!("{} 已绑定当前官方登录态。", account.nickname),
        "real_binding",
        "account_bound",
        None,
    )?;
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    Ok(account)
}

#[tauri::command]
fn verify_bound_account(
    id: i64,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Account, String> {
    let verified = verify_real_codex_session()?;
    let live_snapshot = live_session_snapshot(&verified)?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let account = query_account_by_id(&connection, id)?;

    if !account.is_real_session || account.binding_kind != "codex_cli" {
        return Err("当前仅支持校验通过官方 CLI 绑定的真实账号。".to_string());
    }

    ensure_verify_target_matches_current_login(&account, &verified)?;

    let now = now_text();
    let same_session = account_matches_live_session(&account, &verified, &live_snapshot)?;
    let next_status = if same_session { "healthy" } else { "warning" };
    let next_auth_state = if same_session { "valid" } else { "mismatch" };

    if same_session {
        connection
            .execute(
                "UPDATE accounts
                 SET status = ?1, auth_state = ?2, profile_ref = COALESCE(?3, profile_ref), account_email = COALESCE(?4, account_email),
                     last_verified_at = ?5, last_check_time = ?5, updated_at = ?5
                 WHERE id = ?6",
                params![
                    next_status,
                    next_auth_state,
                    verified.account_id.clone(),
                    verified.account_email.clone(),
                    now,
                    id
                ],
            )
            .map_err(|error| error.to_string())?;
    } else {
        connection
            .execute(
                "UPDATE accounts
                 SET status = ?1, auth_state = ?2, last_verified_at = ?3, last_check_time = ?3, updated_at = ?3
                 WHERE id = ?4",
                params![next_status, next_auth_state, now, id],
            )
            .map_err(|error| error.to_string())?;
    }

    let updated = query_account_by_id(&connection, id)?;
    if same_session {
        insert_account_notification(
            &connection,
            updated.id,
            "success",
            "账号验证通过",
            &format!("{} 的官方登录态校验通过。", updated.nickname),
            "real_verification",
            "verify_passed",
            None,
        )?;
    } else {
        insert_account_notification(
            &connection,
            updated.id,
            "warning",
            "账号验证异常",
            &format!("{} 绑定的会话与当前官方登录态不一致。", updated.nickname),
            "real_verification",
            "verify_mismatch",
            None,
        )?;
    }
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    Ok(updated)
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    reconcile_runtime_active_identity(&connection)?;
    ensure_one_active(&connection)?;
    let overview = dashboard_overview(&connection)?;
    Ok(BootstrapState {
        overview: overview.clone(),
        accounts: overview.accounts.clone(),
        settings: overview.settings.clone(),
    })
}

#[tauri::command]
fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    reconcile_runtime_active_identity(&connection)?;
    ensure_one_active(&connection)?;
    query_accounts(&connection)
}

#[tauri::command]
fn list_credential_profiles(state: State<'_, AppState>) -> Result<Vec<CredentialProfile>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    reconcile_runtime_active_identity(&connection)?;
    query_credential_profiles(&connection)
}

#[tauri::command]
fn get_key_profile_usage(
    profile_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<ThirdPartyKeyUsageSummary>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let profile = query_credential_profile_by_id(&connection, profile_id)?;
    drop(connection);

    if profile.profile_kind != "third_party_key" {
        return Err("只有第三方 key 支持余额查询。".to_string());
    }

    Ok(fetch_third_party_key_usage_summary(&profile))
}

#[tauri::command]
fn update_key_profile_usage_config(
    input: UpdateKeyProfileUsageConfigInput,
    state: State<'_, AppState>,
) -> Result<CredentialProfile, String> {
    let usage_provider_type = normalized_usage_provider_type(input.usage_provider_type.as_deref());
    let usage_access_token = input
        .usage_access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if let Some(token) = usage_access_token.as_deref() {
        validate_usage_access_secret(token)?;
    }

    let (existing_usage_secret_ref, profile_id) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        let existing = query_credential_profile_by_id(&connection, input.profile_id)?;
        if existing.profile_kind != "third_party_key" {
            return Err("只有第三方 key 支持编辑余额统计配置。".to_string());
        }
        (existing.usage_secret_ref.clone(), existing.id)
    };

    let mut next_usage_secret_ref = None;
    let mut next_usage_masked_secret = None;

    if usage_provider_type.as_deref() == Some("new_api") {
        if let Some(token) = usage_access_token.as_deref() {
            let secret_key = existing_usage_secret_ref
                .as_deref()
                .and_then(keychain_account_key)
                .map(ToString::to_string)
                .unwrap_or_else(|| usage_secret_key_for_profile(profile_id));
            let secret_ref = store_account_secret(&secret_key, token)?;
            next_usage_secret_ref = Some(Some(secret_ref));
            next_usage_masked_secret = Some(Some(mask_secret(token)));
        } else if existing_usage_secret_ref.is_some() {
            next_usage_secret_ref = Some(existing_usage_secret_ref.clone());
        }
    } else {
        if let Some(secret_ref) = existing_usage_secret_ref.as_deref() {
            if let Some(account_key) = keychain_account_key(secret_ref) {
                let _ = delete_account_secret(account_key);
            }
        }
        next_usage_secret_ref = Some(None);
        next_usage_masked_secret = Some(None);
    }

    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    update_key_profile_usage_config_record(
        &connection,
        &UpdateKeyProfileUsageConfigInput {
            profile_id: input.profile_id,
            usage_provider_type,
            usage_query_user: input.usage_query_user,
            usage_query_app_version: input.usage_query_app_version,
            usage_access_token: None,
        },
        next_usage_masked_secret,
        next_usage_secret_ref,
    )
}

#[tauri::command]
fn create_key_profile(
    input: CreateKeyProfileInput,
    state: State<'_, AppState>,
) -> Result<CredentialProfile, String> {
    validate_api_key_secret(&input.api_key)?;
    let secret_key = format!(
        "key-profile-{}-{}",
        normalize_account_key(&format!("{}-{}", input.provider, input.nickname)),
        Local::now().timestamp_millis()
    );
    let secret_ref = store_account_secret(&secret_key, input.api_key.trim())?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    create_key_profile_record(&connection, input, &secret_ref)
}

#[tauri::command]
fn update_key_profile(
    input: UpdateKeyProfileInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CredentialProfile, String> {
    let api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if let Some(api_key) = api_key.as_deref() {
        validate_api_key_secret(api_key)?;
    }
    let (secret_ref, masked_secret) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        let existing = query_credential_profile_by_id(&connection, input.id)?;
        if existing.profile_kind != "third_party_key" {
            return Err("只有第三方 key 支持编辑。".to_string());
        }
        let secret_ref = existing
            .secret_ref
            .clone()
            .ok_or_else(|| "第三方 key 缺少 Keychain 引用。".to_string())?;
        let masked_secret = api_key.as_deref().map(mask_secret);
        (secret_ref, masked_secret)
    };

    if let Some(api_key) = api_key.as_deref() {
        let account_key = keychain_account_key(&secret_ref)
            .ok_or_else(|| "第三方 key 的 Keychain 引用格式无效。".to_string())?;
        store_account_secret(account_key, api_key)?;
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let profile = update_key_profile_record(&connection, &input, masked_secret.as_deref())?;
    drop(connection);
    if profile.is_active {
        apply_key_profile_runtime_config(&profile)?;
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        sync_codex_thread_visibility_for_active_owner(&connection)?;
        drop(connection);
        schedule_codex_desktop_reload_if_running(&app);
    }
    Ok(profile)
}

#[tauri::command]
fn activate_credential_profile(
    profile_id: i64,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CredentialProfile, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let profile = activate_credential_profile_record(&connection, profile_id)?;
    let linked_account = if profile.profile_kind == "official_account" {
        profile
            .linked_account_id
            .map(|account_id| query_account_by_id(&connection, account_id))
            .transpose()?
    } else {
        None
    };
    drop(connection);

    match profile.profile_kind.as_str() {
        "third_party_key" => apply_key_profile_runtime_config(&profile)?,
        "official_account" => {
            let account = linked_account.ok_or_else(|| "官方账号资产缺少关联账号。".to_string())?;
            apply_official_account_runtime_config(&account)?;
        }
        _ => {}
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    sync_codex_thread_visibility_for_active_owner(&connection)?;
    drop(connection);
    schedule_codex_desktop_reload_if_running(&app);
    Ok(profile)
}

#[tauri::command]
fn delete_credential_profile(profile_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    reconcile_runtime_active_identity(&connection)?;
    delete_credential_profile_record(&connection, profile_id)
}

#[tauri::command]
fn get_account_detail(id: i64, state: State<'_, AppState>) -> Result<AccountDetail, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    build_account_detail(&connection, id)
}

fn current_login_matches_account(
    current_login: Option<&CurrentCodexLogin>,
    account: &Account,
) -> bool {
    let Some(current_login) = current_login else {
        return false;
    };
    if !current_login.logged_in {
        return false;
    }
    current_login
        .email
        .as_deref()
        .is_some_and(|email| account.account_email.as_deref() == Some(email))
        || current_login
            .account_id
            .as_deref()
            .is_some_and(|account_id| account.profile_ref.as_deref() == Some(account_id))
}

fn delete_account_record(
    connection: &Connection,
    id: i64,
    current_login: Option<&CurrentCodexLogin>,
) -> Result<(), String> {
    let account = query_account_by_id(connection, id)?;
    if account.is_active {
        return Err("当前登录的官方账号不能删除，请先切换到其他身份。".to_string());
    }
    if current_login_matches_account(current_login, &account) {
        return Err("当前登录的官方账号不能删除，请先切换到其他身份。".to_string());
    }

    connection
        .execute("DELETE FROM accounts WHERE id = ?1", [id])
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM credential_profiles
             WHERE profile_kind = 'official_account' AND linked_account_id = ?1",
            [id],
        )
        .map_err(|error| error.to_string())?;

    if account.is_real_session && !account.session_ref.is_empty() {
        delete_session_snapshot(&account.session_ref)?;
    }

    Ok(())
}

#[tauri::command]
fn delete_account(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    reconcile_runtime_active_identity(&connection)?;
    let accounts = query_accounts(&connection)?;
    let current_login = current_codex_login(&accounts);
    delete_account_record(&connection, id, current_login.as_ref())
}

#[tauri::command]
fn set_default_account(id: i64, state: State<'_, AppState>) -> Result<Account, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    connection
        .execute("UPDATE accounts SET is_default = 0", [])
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE accounts SET is_default = 1, updated_at = ?1 WHERE id = ?2",
            params![now_text(), id],
        )
        .map_err(|error| error.to_string())?;
    query_accounts(&connection)?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or_else(|| "默认账号设置失败".to_string())
}

#[tauri::command]
fn repair_account_auth(
    id: i64,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Account, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let account = query_account_by_id(&connection, id)?;
    drop(connection);

    if account.is_real_session {
        if account.binding_kind != "codex_cli" {
            return Err("当前仅支持修复通过官方 CLI 绑定的真实账号。".to_string());
        }

        let verified = verify_real_codex_session()?;
        ensure_verify_target_matches_current_login(&account, &verified)?;
        let live_snapshot = live_session_snapshot(&verified)?;
        let config_dir = codex_config_dir()?;
        write_official_account_runtime_files(&config_dir, &live_snapshot.credentials_json)?;
        let profile_ref = verified.account_id.clone();
        let account_email = verified.account_email.clone();
        let stored_session_ref = if account.session_ref.is_empty() {
            store_account_secret(&account.account_key, &live_snapshot.credentials_json)?
        } else {
            let snapshot = SessionSnapshot {
                session_ref: account.session_ref.clone(),
                credentials_json: live_snapshot.credentials_json,
            };
            restore_session_snapshot(&snapshot)?;
            account.session_ref.clone()
        };

        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        let now = now_text();
        connection
            .execute(
                "UPDATE accounts
                 SET provider = 'Codex', nickname = ?1, status = 'healthy', auth_state = 'valid',
                     binding_kind = 'codex_cli', session_ref = ?2, profile_ref = ?3, account_email = ?4,
                     last_verified_at = ?5, last_check_time = ?5, is_real_session = 1, updated_at = ?5
                 WHERE id = ?6",
                params![account.nickname, stored_session_ref, profile_ref, account_email, now, id],
            )
            .map_err(|error| error.to_string())?;
        let updated = query_account_by_id(&connection, id)?;
        insert_account_notification(
            &connection,
            updated.id,
            "success",
            "账号已重新绑定",
            &format!("{} 已重新绑定到当前官方登录态。", updated.nickname),
            "real_repair",
            "account_repaired",
            None,
        )?;
        let presentation = current_tray_presentation(&connection);
        drop(connection);
        let _ = apply_tray_presentation(&app, &presentation);
        return Ok(updated);
    }

    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    connection
        .execute(
            "UPDATE accounts SET auth_state = 'valid', status = 'healthy', updated_at = ?1 WHERE id = ?2",
            params![now_text(), id],
        )
        .map_err(|error| error.to_string())?;
    let updated = query_accounts(&connection)?
        .into_iter()
        .find(|account| account.id == id)
        .ok_or_else(|| "授权修复后未找到账号".to_string())?;
    insert_account_notification(
        &connection,
        updated.id,
        "success",
        "授权已修复",
        &format!("{} 已恢复可用状态。", updated.nickname),
        "real_repair",
        "auth_repaired",
        None,
    )?;
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    Ok(updated)
}

#[tauri::command]
fn get_dashboard_overview(state: State<'_, AppState>) -> Result<DashboardOverview, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    dashboard_overview(&connection)
}

#[tauri::command]
fn trigger_usage_sampling(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<DashboardOverview, String> {
    let Some(_guard) = try_begin_sampling_run(&state) else {
        return Err("已有采样任务正在运行，请稍后刷新状态。".to_string());
    };

    generate_usage_snapshots_for_state(&state)?;

    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    maybe_enqueue_recovery_notifications(&connection)?;
    let overview = dashboard_overview(&connection)?;
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    Ok(overview)
}

fn enqueue_post_switch_sampling(app_handle: &tauri::AppHandle, target_account_id: i64) {
    let app_handle = app_handle.clone();

    thread::spawn(move || {
        let Some(state) = app_handle.try_state::<AppState>() else {
            return;
        };
        let Some(_guard) = try_begin_sampling_run(&state) else {
            return;
        };

        let account = {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => return,
            };
            match query_account_by_id(&connection, target_account_id) {
                Ok(account) if account.is_real_session => account,
                _ => return,
            }
        };

        let outcome = collect_real_account_sampling_outcome(&account);
        let now = Local::now();
        let connection = match state.db.lock() {
            Ok(connection) => connection,
            Err(_) => return,
        };

        match outcome {
            Ok(outcome) => {
                let mut created_notifications = 0;
                if apply_background_sampling_outcome(
                    &connection,
                    &account,
                    now,
                    outcome,
                    &mut created_notifications,
                    3,
                )
                .is_ok()
                {
                    let _ = insert_account_notification(
                        &connection,
                        target_account_id,
                        "success",
                        "切换后额度已刷新",
                        &format!("{} 已切换成功，真实额度已在后台更新。", account.nickname),
                        "real_switch",
                        "switch_sampled",
                        None,
                    );
                }
            }
            Err(error) => {
                let _ = insert_account_notification(
                    &connection,
                    target_account_id,
                    "warning",
                    "切换后自动采样失败",
                    &format!("账号已切换成功，但后台采样失败：{}", error),
                    "real_switch",
                    "switch_sample_failed",
                    None,
                );
            }
        }

        let _ = maybe_enqueue_recovery_notifications(&connection);
        let presentation = current_tray_presentation(&connection);
        drop(connection);
        let _ = apply_tray_presentation(&app_handle, &presentation);
    });
}

#[tauri::command]
fn switch_account(
    target_account_id: i64,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<DashboardOverview, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let current_active: Option<i64> = connection
        .query_row(
            "SELECT id FROM accounts WHERE is_active = 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    if current_active == Some(target_account_id) {
        return Err("目标账号已经是当前活跃账号，无需重复切换".into());
    }

    let target_account = query_account_by_id(&connection, target_account_id)?;

    if let Err(reason) = is_switchable(&target_account) {
        insert_switch_log(
            &connection,
            current_active,
            target_account_id,
            "failed",
            &reason,
        )?;
        insert_account_notification(
            &connection,
            target_account_id,
            "error",
            "账号切换失败",
            &reason,
            "real_switch",
            "switch_blocked",
            None,
        )?;
        return Err(reason);
    }

    let current_active_account = current_active
        .map(|id| query_account_by_id(&connection, id))
        .transpose()?;

    let live_status_before_switch = if target_account.is_real_session
        || current_active_account
            .as_ref()
            .map(|account| account.is_real_session)
            .unwrap_or(false)
    {
        Some(read_codex_auth_status_cached()?)
    } else {
        None
    };

    let live_snapshot_before_switch = if let Some(status) = live_status_before_switch.as_ref() {
        Some(live_session_snapshot(status)?)
    } else {
        None
    };

    let target_snapshot_before_switch = if target_account.is_real_session {
        let snapshot = read_bound_session_snapshot(&target_account)?;
        if !bound_snapshot_matches_account(&target_account, &snapshot) {
            insert_switch_log(
                &connection,
                current_active,
                target_account_id,
                "failed",
                "目标账号凭证与绑定身份不一致",
            )?;
            insert_account_notification(
                &connection,
                target_account_id,
                "error",
                "账号切换失败",
                "目标账号凭证与绑定身份不一致，请重新登录并重绑。",
                "real_switch",
                "switch_failed",
                None,
            )?;
            return Err("目标账号凭证与绑定身份不一致，请重新登录并重绑。".to_string());
        }
        Some(snapshot)
    } else {
        None
    };

    if let Some(live_snapshot) = live_snapshot_before_switch.as_ref() {
        if let Some(active_account) = current_active_account.as_ref() {
            if live_status_before_switch
                .as_ref()
                .is_some_and(|status| account_matches_verified_identity(active_account, status))
            {
                sync_real_account_snapshot_in_background(
                    active_account.clone(),
                    live_snapshot.clone(),
                );
            }
        }
        if let Some(target_snapshot) = target_snapshot_before_switch.as_ref() {
            restore_real_session_snapshot(target_snapshot, live_snapshot)?;
            let config_dir = codex_config_dir()?;
            write_official_account_runtime_files(&config_dir, &target_snapshot.credentials_json)?;
        }
    }

    set_active_account(&connection, target_account_id)?;

    let live_snapshot_after_switch =
        target_snapshot_before_switch
            .as_ref()
            .and_then(|target_snapshot| {
                live_snapshot_before_switch
                    .as_ref()
                    .map(|live_snapshot| SessionSnapshot {
                        session_ref: live_snapshot.session_ref.clone(),
                        credentials_json: target_snapshot.credentials_json.clone(),
                    })
            });

    if let Err(reason) = validate_post_switch(
        &connection,
        target_account_id,
        live_snapshot_after_switch.as_ref(),
    ) {
        if let Some(snapshot) = live_snapshot_before_switch.as_ref() {
            let _ = restore_session_snapshot(snapshot);
        }
        rollback_active_account(&connection, current_active, ensure_one_active)?;
        insert_switch_log(
            &connection,
            current_active,
            target_account_id,
            "failed",
            &reason,
        )?;
        insert_account_notification(
            &connection,
            target_account_id,
            "error",
            "账号切换失败",
            &reason,
            "real_switch",
            "switch_failed",
            None,
        )?;
        return Err(reason);
    }

    if target_account.is_real_session {
        let now = now_text();
        let (switched_profile_ref, switched_account_email) = live_snapshot_after_switch
            .as_ref()
            .map(|snapshot| extract_codex_identity_from_session_json(&snapshot.credentials_json))
            .unwrap_or((None, None));
        let stored_session_ref = if target_account.session_ref.is_empty() {
            keychain_session_ref(&target_account.account_key)
        } else {
            target_account.session_ref.clone()
        };

        connection
            .execute(
                "UPDATE accounts
                 SET status = 'healthy',
                     auth_state = 'valid',
                     session_ref = ?1,
                     profile_ref = COALESCE(?2, profile_ref),
                     account_email = COALESCE(?3, account_email),
                     last_verified_at = ?4,
                     last_check_time = ?4,
                     updated_at = ?4
                 WHERE id = ?5",
                params![
                    stored_session_ref,
                    switched_profile_ref,
                    switched_account_email,
                    now,
                    target_account_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }

    if target_account.is_real_session {
        activate_account_credential_profile_record(&connection, target_account_id)?;
    }
    sync_codex_thread_visibility_for_active_owner(&connection)?;

    insert_switch_log(
        &connection,
        current_active,
        target_account_id,
        "success",
        "主窗口或 Menubar 发起切换",
    )?;
    insert_account_notification(
        &connection,
        target_account_id,
        "success",
        "账号切换成功",
        &format!("已切换到 {}。", target_account.nickname),
        "real_switch",
        "switch_success",
        None,
    )?;
    let _ = maybe_enqueue_recovery_notifications(&connection);
    let overview = dashboard_overview(&connection)?;
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    enqueue_post_switch_sampling(&app, target_account_id);
    schedule_codex_desktop_reload_if_running(&app);
    Ok(overview)
}

#[tauri::command]
fn list_local_projects(state: State<'_, AppState>) -> Result<Vec<LocalProject>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    query_local_projects(&connection)
}

#[tauri::command]
fn list_session_records(state: State<'_, AppState>) -> Result<Vec<SessionRecord>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    query_session_records(&connection)
}

#[tauri::command]
fn list_sessions_for_profile(
    profile_kind: String,
    profile_ref: String,
    state: State<'_, AppState>,
) -> Result<Vec<SessionRecord>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    backfill_codex_imported_session_titles(&connection)?;
    backfill_codex_imported_session_owners(&connection)?;
    backfill_codex_imported_state_model_providers(&connection)?;
    sync_codex_thread_visibility_for_active_owner(&connection)?;
    let mut stmt = connection
        .prepare(
            "SELECT DISTINCT s.id, s.project_id, p.name, p.workspace_path, s.owner_account_id,
                    s.owner_profile_kind, s.owner_profile_ref, s.record_type, s.title, s.summary,
                    s.raw_content, s.message_count, s.source_record_id, s.created_at, s.updated_at
             FROM session_records s
             JOIN local_projects p ON p.id = s.project_id
             LEFT JOIN session_profile_links l ON l.session_id = s.id
             WHERE (s.owner_profile_kind = ?1 AND s.owner_profile_ref = ?2)
                OR (l.profile_kind = ?1 AND l.profile_ref = ?2)
             ORDER BY s.updated_at DESC, s.id DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![profile_kind, profile_ref], map_session_record)
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_workspace_support_data(state: State<'_, AppState>) -> Result<WorkspaceSupportData, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    query_workspace_support_data(&connection)
}

#[tauri::command]
fn import_codex_local_sessions(
    state: State<'_, AppState>,
) -> Result<CodexLocalSessionImportResult, String> {
    let codex_dir = codex_config_dir()?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let result = import_codex_local_sessions_from_dir(&connection, &codex_dir)?;
    insert_structured_notification(
        &connection,
        None,
        "success",
        "已导入本地 Codex 会话",
        &result.message,
        "system",
        "import_codex_local_sessions",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
fn list_codex_local_session_candidates(
    state: State<'_, AppState>,
) -> Result<Vec<CodexLocalSessionCandidate>, String> {
    let codex_dir = codex_config_dir()?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    list_codex_local_session_candidates_from_dir(&connection, &codex_dir)
}

#[tauri::command]
fn import_codex_local_session_candidates(
    state: State<'_, AppState>,
    candidate_ids: Vec<String>,
) -> Result<CodexLocalSessionImportResult, String> {
    let codex_dir = codex_config_dir()?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let result = import_codex_local_session_candidates_from_dir(
        &connection,
        &codex_dir,
        &candidate_ids,
        true,
    )?;
    insert_structured_notification(
        &connection,
        None,
        "success",
        "已导入选中的本地 Codex 会话",
        &result.message,
        "system",
        "import_codex_local_session_candidates",
        None,
    )?;
    Ok(result)
}

#[tauri::command]
fn list_notifications(state: State<'_, AppState>) -> Result<Vec<NotificationItem>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    query_notifications(&connection)
}

#[tauri::command]
fn get_release_diagnostic(state: State<'_, AppState>) -> Result<ReleaseDiagnostic, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    build_release_diagnostic(&connection)
}

#[tauri::command]
fn get_startup_health(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<StartupHealth, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    Ok(build_startup_health(&connection, &app_dir, now_text()))
}

#[tauri::command]
fn preview_cleanup_debug_data(state: State<'_, AppState>) -> Result<CleanupPreview, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    cleanup_preview(&connection)
}

#[tauri::command]
fn cleanup_debug_data(state: State<'_, AppState>) -> Result<CleanupResult, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    let result = cleanup_historical_debug_data(&connection)?;
    insert_notification(
        &connection,
        "info",
        "历史调试数据已清理",
        &format!("已清理 {} 条历史调试记录。", result.deleted_total),
        "system",
    )?;
    Ok(result)
}

#[tauri::command]
fn update_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AppSettings, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;
    connection
        .execute(
            "UPDATE app_settings SET warn_threshold_low = ?1, warn_threshold_mid = ?2, warn_threshold_high = ?3, check_interval = ?4, enable_handoff = ?5, prefer_official_upgrade = ?6, enable_auto_refresh = ?7, enable_auto_sampling = ?8, foreground_auto_sampling_only = ?9, launch_at_login = ?10, menu_bar_only = ?11 WHERE id = 1",
            params![
                settings.warn_threshold_low,
                settings.warn_threshold_mid,
                settings.warn_threshold_high,
                settings.check_interval,
                settings.enable_handoff as i64,
                settings.prefer_official_upgrade as i64,
                settings.enable_auto_refresh as i64,
                settings.enable_auto_sampling as i64,
                settings.foreground_auto_sampling_only as i64,
                settings.launch_at_login as i64,
                settings.menu_bar_only as i64,
            ],
        )
        .map_err(|error| error.to_string())?;

    if settings.launch_at_login {
        insert_notification(
            &connection,
            "info",
            "开机启动已开启",
            "当前为本地 MVP，占位记录已保存，后续可接系统能力。",
            "settings_event",
        )?;
    }

    insert_notification(
        &connection,
        "info",
        "设置已更新",
        if settings.prefer_official_upgrade {
            "当前仍保持官方扩容优先策略。"
        } else {
            "当前已关闭官方扩容优先策略。"
        },
        "settings_event",
    )?;

    let updated = query_settings(&connection)?;
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(&app, &presentation);
    Ok(updated)
}

#[tauri::command]
fn open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = ensure_main_window(&app)?;
    show_window(&window)?;
    Ok(())
}

fn ensure_main_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window("main") {
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("CodexSwitcher Mac")
        .inner_size(1440.0, 960.0)
        .min_inner_size(1200.0, 760.0)
        .resizable(true)
        .visible(true)
        .build()
        .map_err(|error| error.to_string())
}

fn show_window(window: &WebviewWindow) -> Result<(), String> {
    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn should_run_background_sampling(app_handle: &tauri::AppHandle, settings: &AppSettings) -> bool {
    if !settings.enable_auto_sampling {
        return false;
    }

    if !settings.foreground_auto_sampling_only {
        return true;
    }

    app_handle
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn parse_local_datetime_text(value: &str) -> Option<chrono::DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").ok()?;
    Local.from_local_datetime(&naive).single()
}

fn escape_osascript_text(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn send_macos_notification(title: &str, message: &str) {
    if cfg!(test) {
        return;
    }
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape_osascript_text(message),
        escape_osascript_text(title),
    );
    let _ = Command::new("osascript").args(["-e", &script]).output();
}

fn insert_once_notification(
    connection: &Connection,
    account_id: Option<i64>,
    level: &str,
    title: &str,
    message: &str,
    source_type: &str,
    action_type: &str,
    related_handoff_id: Option<i64>,
) -> Result<bool, String> {
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM notifications
             WHERE account_id IS ?1 AND title = ?2 AND message = ?3 AND source_type = ?4 AND action_type = ?5",
            params![account_id, title, message, source_type, action_type],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    if exists > 0 {
        return Ok(false);
    }

    insert_structured_notification(
        connection,
        account_id,
        level,
        title,
        message,
        source_type,
        action_type,
        related_handoff_id,
    )?;
    Ok(true)
}

fn maybe_enqueue_recovery_notifications(connection: &Connection) -> Result<(), String> {
    let now = Local::now();
    let accounts = query_accounts(connection)?;

    for account in accounts {
        let Some(snapshot) = account.latest_snapshot.clone() else {
            continue;
        };

        let windows = [
            (
                "5h",
                snapshot.window_5h_percent,
                snapshot.estimated_reset_5h_at.clone(),
                "recovery_soon_5h",
            ),
            (
                "7d",
                snapshot.window_7d_percent,
                snapshot.estimated_reset_7d_at.clone(),
                "recovery_soon_7d",
            ),
        ];

        for (window_label, percent, reset_at, action_type) in windows {
            if percent < 100 {
                continue;
            }

            let Some(reset_at_text) = reset_at else {
                continue;
            };
            let Some(reset_at_time) = parse_local_datetime_text(&reset_at_text) else {
                continue;
            };
            let minutes_until = (reset_at_time - now).num_minutes();
            if !(0..=RECOVERY_REMINDER_MINUTES).contains(&minutes_until) {
                continue;
            }

            let title = format!("{} 即将恢复", account.nickname);
            let message = format!(
                "{} 的 {} 窗口预计将在 {} 恢复，可提前准备切换或采样。",
                account.nickname, window_label, reset_at_text
            );

            if insert_once_notification(
                connection,
                Some(account.id),
                "info",
                &title,
                &message,
                "system",
                action_type,
                None,
            )? {
                send_macos_notification(&title, &message);
            }
        }
    }

    Ok(())
}

fn run_background_sampling_pass(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    last_error_message: &mut Option<String>,
) -> Result<(), String> {
    let accounts = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;
        automatic_sampling_accounts(&query_accounts(&connection)?, Local::now())
    };

    let mut created_notifications = 0;
    let mut failures = Vec::new();

    for account in &accounts {
        let outcome = collect_real_account_sampling_outcome(account);
        let now = Local::now();
        let connection = state
            .db
            .lock()
            .map_err(|_| "数据库锁获取失败".to_string())?;

        match outcome {
            Ok(outcome) => {
                apply_background_sampling_outcome(
                    &connection,
                    account,
                    now,
                    outcome,
                    &mut created_notifications,
                    3,
                )?;
            }
            Err(error) => failures.push(format!("{}：{}", account.nickname, error)),
        }
    }

    let connection = state
        .db
        .lock()
        .map_err(|_| "数据库锁获取失败".to_string())?;

    if failures.is_empty() {
        let _ = maybe_enqueue_recovery_notifications(&connection);
        let presentation = current_tray_presentation(&connection);
        drop(connection);
        let _ = apply_tray_presentation(app_handle, &presentation);
        *last_error_message = None;
        return Ok(());
    }

    let message = format!(
        "后台自动采样失败：{}",
        summarize_sampling_failures(&failures)
    );
    if last_error_message.as_deref() != Some(message.as_str()) {
        let _ = insert_once_notification(
            &connection,
            None,
            "error",
            "后台自动采样失败",
            &message,
            "system",
            "background_sample_failed",
            None,
        );
        *last_error_message = Some(message.clone());
    }
    let presentation = current_tray_presentation(&connection);
    drop(connection);
    let _ = apply_tray_presentation(app_handle, &presentation);
    Err(message)
}

fn setup_background_sampler(app: &tauri::App) {
    let app_handle = app.handle().clone();

    thread::spawn(move || {
        let mut last_run_at = std::time::Instant::now()
            .checked_sub(StdDuration::from_secs(300))
            .unwrap_or_else(std::time::Instant::now);
        let mut last_error_message: Option<String> = None;

        loop {
            let mut sleep_secs = 15;

            if let Some(state) = app_handle.try_state::<AppState>() {
                let should_run = if let Ok(connection) = state.db.lock() {
                    if let Ok(settings) = query_settings(&connection) {
                        sleep_secs = settings.check_interval.max(10) as u64;
                        should_run_background_sampling(&app_handle, &settings)
                            && last_run_at.elapsed() >= StdDuration::from_secs(sleep_secs)
                    } else {
                        false
                    }
                } else {
                    false
                };

                if should_run {
                    if let Some(_guard) = try_begin_sampling_run(&state) {
                        let _ = run_background_sampling_pass(
                            &state,
                            &app_handle,
                            &mut last_error_message,
                        );
                        last_run_at = std::time::Instant::now();
                    }
                }
            }

            thread::sleep(StdDuration::from_secs(sleep_secs.min(30)));
        }
    });
}

struct TrayPresentation {
    title: String,
    tooltip: String,
    detail: String,
    sampling: String,
    alert: Option<String>,
    can_sample: bool,
}

fn format_usage_balance(value: Option<f64>, unit: Option<&str>) -> Option<String> {
    let value = value?;
    let unit = unit.unwrap_or("USD").trim();
    if unit.is_empty() {
        return Some(format!("{value:.2}"));
    }
    Some(format!("{unit} {value:.2}"))
}

fn tray_status_prefix(status: &str) -> &'static str {
    match status {
        "exhausted" | "auth_invalid" | "error" => "⚠",
        "warning" => "△",
        _ => "",
    }
}

fn tray_status_text(status: &str) -> &'static str {
    match status {
        "healthy" => "健康",
        "warning" => "预警",
        "exhausted" => "不可用",
        "auth_invalid" => "登录失效",
        _ => "异常",
    }
}

fn active_key_credential_profile(
    connection: &Connection,
) -> Result<Option<CredentialProfile>, String> {
    connection
        .query_row(
            "SELECT id, profile_kind, provider, nickname, status, is_active,
                    base_url, model, masked_secret, secret_ref, linked_account_id,
                    usage_provider_type, usage_query_user, usage_query_app_version,
                    usage_masked_secret, usage_secret_ref
             FROM credential_profiles
             WHERE profile_kind = 'third_party_key' AND is_active = 1
             LIMIT 1",
            [],
            |row| {
                Ok(CredentialProfile {
                    id: row.get(0)?,
                    profile_kind: row.get(1)?,
                    provider: row.get(2)?,
                    nickname: row.get(3)?,
                    status: row.get(4)?,
                    is_active: row.get::<_, i64>(5)? == 1,
                    base_url: row.get(6)?,
                    model: row.get(7)?,
                    masked_secret: row.get(8)?,
                    secret_ref: row.get(9)?,
                    linked_account_id: row.get(10)?,
                    usage_provider_type: row.get(11)?,
                    usage_query_user: row.get(12)?,
                    usage_query_app_version: row.get(13)?,
                    usage_masked_secret: row.get(14)?,
                    usage_secret_ref: row.get(15)?,
                    usage_summary: None,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn key_tray_presentation(profile: &CredentialProfile) -> TrayPresentation {
    let model = profile.model.as_deref().unwrap_or("未设置模型");
    let base_url = profile.base_url.as_deref().unwrap_or("未设置 Base URL");
    let masked_secret = profile.masked_secret.as_deref().unwrap_or("未保存 key");
    let usage = fetch_third_party_key_usage_summary(profile);
    let balance_text = usage.as_ref().and_then(|summary| {
        format_usage_balance(
            summary.remaining.or(summary.balance),
            summary.unit.as_deref(),
        )
    });
    let title = balance_text
        .clone()
        .unwrap_or_else(|| format!("Key {}", profile.nickname));
    let detail = balance_text
        .clone()
        .map(|text| format!("当前余额：{text}"))
        .unwrap_or_else(|| format!("当前：Key · {} · {}", profile.nickname, model));
    let tooltip = if let Some(text) = balance_text {
        format!(
            "{}｜{}｜{}｜{}",
            profile.nickname, text, profile.provider, base_url
        )
    } else {
        format!(
            "{}｜Key｜{}｜{}｜{}｜{}",
            profile.nickname, profile.provider, model, base_url, masked_secret
        )
    };

    TrayPresentation {
        title,
        tooltip,
        detail,
        sampling: "Key 身份无需采样".to_string(),
        alert: None,
        can_sample: false,
    }
}

fn current_tray_presentation(connection: &Connection) -> TrayPresentation {
    match dashboard_overview(connection) {
        Ok(overview) => {
            if let Ok(Some(profile)) = active_key_credential_profile(connection) {
                return key_tray_presentation(&profile);
            }

            if let Some(active) = overview.active_account {
                let alert = match active.status.as_str() {
                    "exhausted" => {
                        Some("当前活跃账号不可用，需等待恢复或切换其他账号。".to_string())
                    }
                    "auth_invalid" => Some("当前活跃账号登录失效，请重新登录并重绑。".to_string()),
                    "warning" => Some("当前活跃账号处于预警区，建议关注恢复时间。".to_string()),
                    _ => None,
                };

                if let Some(snapshot) = overview.latest_snapshot {
                    let prefix = tray_status_prefix(&snapshot.risk_level);
                    let window_5h_text = usage_percent_text(snapshot.window_5h_percent);
                    let window_7d_text = usage_percent_text(snapshot.window_7d_percent);
                    let title = if prefix.is_empty() {
                        format!("{} {}/{}", active.nickname, window_5h_text, window_7d_text)
                    } else {
                        format!(
                            "{} {} {}/{}",
                            prefix, active.nickname, window_5h_text, window_7d_text
                        )
                    };
                    let tooltip = format!(
                        "{}｜状态 {}｜5h剩余 {}｜7d剩余 {}｜5h恢复 {}｜7d恢复 {}",
                        active.nickname,
                        tray_status_text(&active.status),
                        window_5h_text,
                        window_7d_text,
                        snapshot.estimated_reset_5h_at.as_deref().unwrap_or("未知"),
                        snapshot.estimated_reset_7d_at.as_deref().unwrap_or("未知"),
                    );
                    let detail = format!(
                        "当前：{} · 状态 {} · 5h剩余 {} · 7d剩余 {}",
                        active.nickname,
                        tray_status_text(&active.status),
                        window_5h_text,
                        window_7d_text,
                    );
                    let sampling = format!("最近采样：{}", snapshot.sample_time);
                    return TrayPresentation {
                        title,
                        tooltip,
                        detail,
                        sampling,
                        alert,
                        can_sample: true,
                    };
                }

                return TrayPresentation {
                    title: active.nickname.clone(),
                    tooltip: format!("{}｜等待真实采样", active.nickname),
                    detail: format!("当前：{} · 等待真实采样", active.nickname),
                    sampling: "最近采样：暂无".to_string(),
                    alert,
                    can_sample: true,
                };
            }

            TrayPresentation {
                title: "CodexSwitcher".to_string(),
                tooltip: "当前：未设置身份".to_string(),
                detail: "当前：未设置身份".to_string(),
                sampling: "最近采样：暂无".to_string(),
                alert: None,
                can_sample: true,
            }
        }
        Err(_) => TrayPresentation {
            title: "⚠ CodexSwitcher".to_string(),
            tooltip: "当前：状态读取失败".to_string(),
            detail: "当前：状态读取失败".to_string(),
            sampling: "最近采样：读取失败".to_string(),
            alert: Some("数据库或状态读取失败，请打开主窗口检查。".to_string()),
            can_sample: true,
        },
    }
}

fn build_tray_menu(
    app_handle: &tauri::AppHandle,
    presentation: &TrayPresentation,
) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    let open = MenuItemBuilder::with_id("open", "打开主窗口")
        .build(app_handle)
        .map_err(|error| error.to_string())?;
    let sample = MenuItemBuilder::with_id("sample", "立即采样")
        .enabled(presentation.can_sample)
        .build(app_handle)
        .map_err(|error| error.to_string())?;
    let settings = MenuItemBuilder::with_id("settings", "打开设置")
        .build(app_handle)
        .map_err(|error| error.to_string())?;
    let project_sessions = MenuItemBuilder::with_id("project_sessions", "查看项目会话")
        .build(app_handle)
        .map_err(|error| error.to_string())?;
    let quit = MenuItemBuilder::with_id("quit", "退出")
        .build(app_handle)
        .map_err(|error| error.to_string())?;

    let mut builder = MenuBuilder::new(app_handle)
        .text("status", &presentation.title)
        .text("detail", &presentation.detail)
        .text("sampling", &presentation.sampling);

    if let Some(alert) = &presentation.alert {
        builder = builder.text("alert", alert);
    }

    builder
        .separator()
        .items(&[&open, &sample, &settings, &project_sessions, &quit])
        .build()
        .map_err(|error| error.to_string())
}

fn apply_tray_presentation(
    app_handle: &tauri::AppHandle,
    presentation: &TrayPresentation,
) -> Result<(), String> {
    let tray = app_handle
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "托盘实例不存在".to_string())?;
    tray.set_title(Some(&presentation.title))
        .map_err(|error| error.to_string())?;
    tray.set_tooltip(Some(&presentation.tooltip))
        .map_err(|error| error.to_string())?;
    let menu = build_tray_menu(app_handle, &presentation)?;
    tray.set_menu(Some(menu))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn setup_tray(app: &tauri::App) -> Result<(), String> {
    let app_state = app.state::<AppState>();
    let presentation = if let Ok(connection) = app_state.db.lock() {
        current_tray_presentation(&connection)
    } else {
        TrayPresentation {
            title: "⚠ CodexSwitcher".to_string(),
            tooltip: "当前：状态读取失败".to_string(),
            detail: "当前：状态读取失败".to_string(),
            sampling: "最近采样：读取失败".to_string(),
            alert: Some("数据库锁获取失败，请打开主窗口检查。".to_string()),
            can_sample: true,
        }
    };
    let menu = build_tray_menu(&app.handle().clone(), &presentation)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(&presentation.tooltip)
        .title(&presentation.title)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| "缺少应用图标".to_string())?,
        )
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { .. } = event {
                if let Ok(window) = ensure_main_window(tray.app_handle()) {
                    let _ = show_window(&window);
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                if let Ok(window) = ensure_main_window(app) {
                    let _ = show_window(&window);
                }
            }
            "sample" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(_guard) = try_begin_sampling_run(&state) {
                        let _ = generate_usage_snapshots_for_state(&state);
                        if let Ok(connection) = state.db.lock() {
                            let _ = maybe_enqueue_recovery_notifications(&connection);
                        }
                    }
                }
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(connection) = state.db.lock() {
                        let presentation = current_tray_presentation(&connection);
                        drop(connection);
                        let _ = apply_tray_presentation(app, &presentation);
                    }
                }
            }
            "settings" => {
                if let Ok(window) = ensure_main_window(app) {
                    let _ = show_window(&window);
                    let _ = window.eval("window.location.hash = '#settings'");
                }
            }
            "project_sessions" => {
                if let Ok(window) = ensure_main_window(app) {
                    let _ = show_window(&window);
                    let _ = window.eval("window.location.hash = '#handoff'");
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::usage_risk_from_windows;
    use crate::usage::AccountStatus;
    use chrono::Duration;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEED_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn setup_test_connection() -> Connection {
        let connection = Connection::open_in_memory().expect("open memory db");
        init_database(&connection).expect("init db");
        let seed_id = TEST_SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
        seed_test_real_accounts(&connection, seed_id);
        connection
    }

    fn active_account_id(connection: &Connection) -> Option<i64> {
        connection
            .query_row(
                "SELECT id FROM accounts WHERE is_active = 1 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok()
    }

    fn unique_temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codexswitcher-test-{}-{}",
            std::process::id(),
            name
        ))
    }

    fn create_codex_state_db(root: &Path) -> Connection {
        fs::create_dir_all(root).expect("create codex root");
        let state = Connection::open(root.join("state_5.sqlite")).expect("open codex state db");
        state
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    rollout_path TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    source TEXT NOT NULL,
                    model_provider TEXT NOT NULL,
                    cwd TEXT NOT NULL,
                    title TEXT NOT NULL,
                    sandbox_policy TEXT NOT NULL,
                    approval_mode TEXT NOT NULL,
                    tokens_used INTEGER NOT NULL DEFAULT 0,
                    has_user_event INTEGER NOT NULL DEFAULT 0,
                    archived INTEGER NOT NULL DEFAULT 0,
                    archived_at INTEGER,
                    git_sha TEXT,
                    git_branch TEXT,
                    git_origin_url TEXT,
                    cli_version TEXT NOT NULL DEFAULT '',
                    first_user_message TEXT NOT NULL DEFAULT '',
                    agent_nickname TEXT,
                    agent_role TEXT,
                    memory_mode TEXT NOT NULL DEFAULT 'enabled',
                    model TEXT,
                    reasoning_effort TEXT,
                    agent_path TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER,
                    thread_source TEXT,
                    preview TEXT NOT NULL DEFAULT '',
                    recency_at INTEGER NOT NULL DEFAULT 0,
                    recency_at_ms INTEGER NOT NULL DEFAULT 0,
                    project_id TEXT
                );",
            )
            .expect("create threads table");
        state
    }

    fn seed_test_real_accounts(connection: &Connection, seed_id: u64) {
        let now = now_text();
        for (index, nickname) in ["账号 A", "账号 B", "账号 C"].into_iter().enumerate() {
            let id = index as i64 + 1;
            let session_path = unique_temp_file(&format!("seed-{}-account-{}.json", seed_id, id));
            fs::write(
                &session_path,
                format!("{{\"loggedIn\":true,\"account\":\"{}\"}}", nickname),
            )
            .expect("write test session");
            connection
                .execute(
                    "INSERT INTO accounts (id, provider, nickname, status, is_active, is_default, auth_state, last_check_time, estimated_reset_time, account_key, binding_kind, session_ref, profile_ref, account_email, last_verified_at, is_real_session, created_at, updated_at)
                     VALUES (?1, 'Codex', ?2, 'healthy', ?3, ?3, 'valid', ?4, NULL, ?5, 'codex_cli', ?6, NULL, NULL, ?4, 1, ?4, ?4)",
                    params![
                        id,
                        nickname,
                        if id == 1 { 1 } else { 0 },
                        now,
                        format!("test-account-{}-{}", seed_id, id),
                        session_path.to_string_lossy().to_string(),
                    ],
                )
                .expect("insert test account");
        }

        for step in [1_i64, 0] {
            let sample_at = (Local::now() - Duration::hours(step))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            connection
                .execute(
                    "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                     VALUES (1, ?1, ?2, 18, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"test\"}')",
                    params![sample_at, if step == 1 { 24 } else { 31 }],
                )
                .expect("insert test real snapshot");
        }

        connection
            .execute(
                "INSERT INTO local_projects (id, name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES (1, '测试项目', '/tmp/codexswitcher-test-project', NULL, ?1, ?1, ?1)",
                [now.clone()],
            )
            .expect("insert test project");
        connection
            .execute(
                "INSERT INTO session_records (project_id, owner_account_id, owner_profile_kind, owner_profile_ref, record_type, title, summary, raw_content, message_count, source_record_id, created_at, updated_at)
                 VALUES (1, 1, 'official_account', 'account:1', 'local_session', '测试会话', '验证本地项目会话链路', '', 0, NULL, ?1, ?1)",
                [now.clone()],
            )
            .expect("insert test session");

        insert_notification(
            connection,
            "info",
            "测试通知",
            "真实账号测试通知",
            "real_verification",
        )
        .expect("insert test notification");
    }

    #[test]
    fn keychain_session_ref_roundtrip() {
        let session_ref = keychain_session_ref("codex-demo");
        assert_eq!(session_ref, "keychain://codex-demo");
        assert_eq!(keychain_account_key(&session_ref), Some("codex-demo"));
        assert_eq!(keychain_account_key("/tmp/demo.json"), None);
    }

    #[test]
    fn decode_hex_payload_restores_json_text() {
        let decoded =
            decode_hex_payload_if_needed("7b226c6f67676564496e223a747275657d".to_string());
        assert_eq!(decoded, "{\"loggedIn\":true}");

        let plain = decode_hex_payload_if_needed("{\"loggedIn\":true}".to_string());
        assert_eq!(plain, "{\"loggedIn\":true}");
    }

    #[test]
    fn legacy_file_session_snapshot_still_supported() {
        let path = unique_temp_file("legacy-read.json");
        fs::write(&path, "{\"token\":\"demo\"}").expect("write legacy session");

        let snapshot =
            read_session_snapshot(path.to_str().expect("path str")).expect("read legacy snapshot");
        assert_eq!(snapshot.credentials_json, "{\"token\":\"demo\"}");

        let updated = SessionSnapshot {
            session_ref: path.to_string_lossy().to_string(),
            credentials_json: "{\"token\":\"next\"}".into(),
        };
        restore_session_snapshot(&updated).expect("restore legacy snapshot");

        let restored = fs::read_to_string(&path).expect("read restored file");
        assert_eq!(restored, "{\"token\":\"next\"}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn delete_session_snapshot_removes_legacy_file() {
        let path = unique_temp_file("legacy-delete.json");
        fs::write(&path, "demo").expect("write file");
        delete_session_snapshot(path.to_str().expect("path str")).expect("delete legacy session");
        assert!(!path.exists());
    }

    #[test]
    fn migrate_legacy_session_ref_skips_keychain_ref() {
        let connection = setup_test_connection();
        let session_ref = keychain_session_ref("codex-demo");

        connection
            .execute(
                "UPDATE accounts SET account_key = ?1, binding_kind = 'codex_cli', is_real_session = 1, session_ref = ?2 WHERE id = 1",
                params!["codex-demo", session_ref.clone()],
            )
            .expect("seed keychain session ref");

        migrate_legacy_session_ref_for_account(&connection, 1, "codex-demo", &session_ref)
            .expect("skip keychain ref");

        let stored: String = connection
            .query_row("SELECT session_ref FROM accounts WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("query session_ref");
        assert_eq!(stored, session_ref);
    }

    #[test]
    fn migrate_legacy_session_ref_skips_missing_file() {
        let connection = setup_test_connection();
        let missing = unique_temp_file("missing-session.json");
        let missing_ref = missing.to_string_lossy().to_string();

        connection
            .execute(
                "UPDATE accounts SET account_key = ?1, binding_kind = 'codex_cli', is_real_session = 1, session_ref = ?2 WHERE id = 1",
                params!["codex-missing", missing_ref],
            )
            .expect("seed legacy session ref");

        migrate_legacy_session_refs(&connection).expect("skip missing legacy file");

        let stored: String = connection
            .query_row("SELECT session_ref FROM accounts WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("query session_ref");
        assert_eq!(stored, missing.to_string_lossy().to_string());
    }

    #[test]
    fn switchable_account_validation_blocks_invalid_states() {
        let invalid_auth = Account {
            id: 1,
            provider: "Codex".into(),
            nickname: "账号 A".into(),
            status: "healthy".into(),
            is_active: false,
            is_default: false,
            auth_state: "expired".into(),
            last_check_time: None,
            estimated_reset_time: None,
            account_key: "seed-a".into(),
            binding_kind: "manual".into(),
            session_ref: "seed://1".into(),
            profile_ref: None,
            account_email: None,
            last_verified_at: None,
            is_real_session: false,
            plan_label: None,
            latest_snapshot: None,
        };
        assert!(is_switchable(&invalid_auth).is_err());

        let exhausted = Account {
            auth_state: "valid".into(),
            status: "exhausted".into(),
            ..invalid_auth.clone()
        };
        assert!(is_switchable(&exhausted).is_err());
    }

    #[test]
    fn bound_snapshot_match_respects_expected_profile_ref() {
        let account = Account {
            id: 1,
            provider: "Codex".into(),
            nickname: "账号 A".into(),
            status: "healthy".into(),
            is_active: false,
            is_default: false,
            auth_state: "valid".into(),
            last_check_time: None,
            estimated_reset_time: None,
            account_key: "codex-00000000-0000-4000-8000-000000000001".into(),
            binding_kind: "codex_cli".into(),
            session_ref: "keychain://codex-00000000-0000-4000-8000-000000000001".into(),
            profile_ref: Some("00000000-0000-4000-8000-000000000001".into()),
            account_email: Some("demo2027@example.com".into()),
            last_verified_at: None,
            is_real_session: true,
            plan_label: None,
            latest_snapshot: None,
        };

        let matching_snapshot = SessionSnapshot {
            session_ref: account.session_ref.clone(),
            credentials_json:
                "{\"tokens\":{\"account_id\":\"00000000-0000-4000-8000-000000000001\"}}".into(),
        };
        assert!(bound_snapshot_matches_account(&account, &matching_snapshot));

        let mismatched_snapshot = SessionSnapshot {
            session_ref: account.session_ref.clone(),
            credentials_json:
                "{\"tokens\":{\"account_id\":\"00000000-0000-4000-8000-000000000002\"}}".into(),
        };
        assert!(!bound_snapshot_matches_account(
            &account,
            &mismatched_snapshot
        ));
    }

    #[test]
    fn verify_target_must_match_current_login() {
        let account = Account {
            id: 1,
            provider: "Codex".into(),
            nickname: "2027".into(),
            status: "healthy".into(),
            is_active: false,
            is_default: false,
            auth_state: "valid".into(),
            last_check_time: None,
            estimated_reset_time: None,
            account_key: "codex-00000000-0000-4000-8000-000000000001".into(),
            binding_kind: "codex_cli".into(),
            session_ref: "keychain://codex-00000000-0000-4000-8000-000000000001".into(),
            profile_ref: Some("00000000-0000-4000-8000-000000000001".into()),
            account_email: Some("demo2027@example.com".into()),
            last_verified_at: None,
            is_real_session: true,
            plan_label: None,
            latest_snapshot: None,
        };

        let matching_login = CodexAuthStatus {
            logged_in: true,
            auth_mode: Some("chatgpt".into()),
            account_id: Some("00000000-0000-4000-8000-000000000001".into()),
            account_email: Some("demo2027@example.com".into()),
            config_dir: "/tmp/codex".into(),
            auth_path: None,
            session_ref: None,
            session_json: "{}".into(),
        };
        assert!(ensure_verify_target_matches_current_login(&account, &matching_login).is_ok());

        let mismatched_login = CodexAuthStatus {
            account_id: Some("00000000-0000-4000-8000-000000000003".into()),
            account_email: Some("demo-gmail@example.com".into()),
            ..matching_login
        };
        let reason = ensure_verify_target_matches_current_login(&account, &mismatched_login)
            .expect_err("mismatched login should be blocked before mutating account");
        assert!(reason.contains("不是"));
        assert!(reason.contains("demo2027@example.com"));
    }

    #[test]
    fn notifications_schema_supports_account_trace_fields() {
        let connection = setup_test_connection();
        for column in ["account_id", "action_type", "related_handoff_id"] {
            let exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('notifications') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .expect("query notification column");
            assert_eq!(exists, 1, "missing notifications.{}", column);
        }
    }

    #[test]
    fn settings_schema_supports_auto_sampling_flags() {
        let connection = setup_test_connection();
        for column in [
            "enable_auto_refresh",
            "enable_auto_sampling",
            "foreground_auto_sampling_only",
        ] {
            let exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('app_settings') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .expect("query settings column");
            assert_eq!(exists, 1, "missing app_settings.{}", column);
        }

        let settings = query_settings(&connection).expect("settings");
        assert!(settings.enable_auto_refresh);
        assert!(settings.enable_auto_sampling);
        assert!(!settings.foreground_auto_sampling_only);
    }

    #[test]
    fn credential_profile_schema_supports_third_party_keys() {
        let connection = setup_test_connection();

        let table_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'credential_profiles'",
                [],
                |row| row.get(0),
            )
            .expect("query credential_profiles table");
        assert_eq!(table_exists, 1);

        for column in [
            "profile_kind",
            "provider",
            "nickname",
            "status",
            "is_active",
            "base_url",
            "model",
            "masked_secret",
            "secret_ref",
            "linked_account_id",
            "usage_provider_type",
            "usage_query_user",
            "usage_query_app_version",
            "usage_masked_secret",
            "usage_secret_ref",
        ] {
            let exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('credential_profiles') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .expect("query credential_profiles column");
            assert_eq!(exists, 1, "missing credential_profiles.{column}");
        }
    }

    #[test]
    fn official_accounts_are_mirrored_as_credential_profiles_once() {
        let connection = setup_test_connection();

        sync_account_credential_profiles(&connection).expect("first sync");
        sync_account_credential_profiles(&connection).expect("second sync");

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE profile_kind = 'official_account'",
                [],
                |row| row.get(0),
            )
            .expect("count official profiles");
        assert_eq!(count, 3);

        let linked: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE linked_account_id = 1 AND nickname = '账号 A'",
                [],
                |row| row.get(0),
            )
            .expect("count linked profile");
        assert_eq!(linked, 1);
    }

    #[test]
    fn third_party_key_profile_masks_secret_and_stores_keychain_ref() {
        let connection = setup_test_connection();
        let input = CreateKeyProfileInput {
            provider: "custom".to_string(),
            nickname: "YuChat 备用 Key".to_string(),
            base_url: "https://sub2api.yuchat.top".to_string(),
            model: "gpt-5-codex".to_string(),
            api_key: "example-api-key-1234567890abcdef".to_string(),
        };

        let profile = create_key_profile_record(&connection, input, "keychain://key-profile-test")
            .expect("create key profile");

        assert_eq!(profile.profile_kind, "third_party_key");
        assert_eq!(profile.provider, "custom");
        assert_eq!(profile.masked_secret, Some("exam...cdef".to_string()));
        assert_eq!(
            profile.secret_ref,
            Some("keychain://key-profile-test".to_string())
        );
    }

    #[test]
    fn api_key_secret_rejects_url_values() {
        assert!(validate_api_key_secret("example-real-secret").is_ok());
        assert_eq!(
            validate_api_key_secret("https://sub2api.yuchat.top"),
            Err("API Key 不能填写 Base URL，请填写供应商后台生成的真实 key。".to_string())
        );
    }

    #[test]
    fn third_party_usage_endpoint_appends_v1_usage() {
        assert_eq!(
            third_party_usage_endpoint_from_base_url("https://sub2api.yuchat.top")
                .expect("usage endpoint"),
            "https://sub2api.yuchat.top/v1/usage"
        );
        assert_eq!(
            third_party_usage_endpoint_from_base_url("https://sub2api.yuchat.top/v1")
                .expect("usage endpoint"),
            "https://sub2api.yuchat.top/v1/usage"
        );
    }

    #[test]
    fn third_party_usage_payload_maps_balance_and_stats() {
        let payload = serde_json::from_str::<ThirdPartyKeyUsageApiResponse>(
            r#"{
                "balance": 136.24714015,
                "isValid": true,
                "mode": "unrestricted",
                "model_stats": [
                    {
                        "model": "gpt-5.4",
                        "requests": 3655,
                        "input_tokens": 21553299,
                        "output_tokens": 3043140,
                        "cache_creation_tokens": 0,
                        "cache_read_tokens": 182251520,
                        "total_tokens": 206847959,
                        "cost": 145.0932275,
                        "actual_cost": 251.46659,
                        "account_cost": 145.0932275
                    }
                ],
                "planName": "钱包余额",
                "remaining": 136.24714015,
                "unit": "USD",
                "usage": {
                    "average_duration_ms": 18968.198441895693,
                    "rpm": 0,
                    "today": {
                        "actual_cost": 186.6273366,
                        "cache_creation_tokens": 0,
                        "cache_read_tokens": 117390208,
                        "cost": 93.3136683,
                        "input_tokens": 5111031,
                        "output_tokens": 315340,
                        "requests": 963,
                        "total_tokens": 122816579
                    },
                    "total": {
                        "actual_cost": 437.73630735,
                        "cache_creation_tokens": 0,
                        "cache_read_tokens": 299514496,
                        "cost": 238.247032,
                        "input_tokens": 26638590,
                        "output_tokens": 3357106,
                        "requests": 4621,
                        "total_tokens": 329510192
                    },
                    "tpm": 0
                }
            }"#,
        )
        .expect("parse usage payload");

        let summary = build_third_party_key_usage_summary(
            "https://sub2api.yuchat.top/v1/usage".to_string(),
            payload,
        );

        assert_eq!(summary.status, "ready");
        assert_eq!(summary.balance, Some(136.24714015));
        assert_eq!(summary.remaining, Some(136.24714015));
        assert_eq!(summary.is_valid, Some(true));
        assert_eq!(summary.plan_name.as_deref(), Some("钱包余额"));
        assert_eq!(summary.unit.as_deref(), Some("USD"));
        assert_eq!(summary.today.as_ref().map(|item| item.requests), Some(963));
        assert_eq!(summary.total.as_ref().map(|item| item.requests), Some(4621));
        assert_eq!(summary.model_stats.len(), 1);
        assert_eq!(summary.model_stats[0].model, "gpt-5.4");
        assert_eq!(summary.usage_provider_type.as_deref(), Some("sub2api"));
        assert!(!summary.detail_items.is_empty());
    }

    #[test]
    fn new_api_payload_maps_quota_and_request_count() {
        let payload = serde_json::from_str::<NewApiUserSelfResponse>(
            r#"{
                "data": {
                    "display_name": "1934829400",
                    "group": "default",
                    "id": 123,
                    "quota": 216449639,
                    "request_count": 1879,
                    "status": 1,
                    "used_quota": 390550361,
                    "username": "1934829400"
                },
                "message": "",
                "success": true
            }"#,
        )
        .expect("parse new api payload");

        let summary = build_new_api_usage_summary(
            "https://www.onetopai.asia/api/user/self".to_string(),
            "123",
            payload,
        )
        .expect("build new api usage summary");

        assert_eq!(summary.usage_provider_type.as_deref(), Some("new_api"));
        assert_eq!(summary.balance, Some(432.899278));
        assert_eq!(summary.unit.as_deref(), Some("USD"));
        assert_eq!(summary.total.as_ref().map(|item| item.requests), Some(1879));
        assert!(summary
            .detail_items
            .iter()
            .any(|item| item.label == "已用金额" && item.value == "USD 781.10"));
    }

    #[test]
    fn updating_key_profile_keeps_existing_secret_when_key_blank() {
        let connection = setup_test_connection();
        let profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "旧 Key".to_string(),
                base_url: "https://old.example.com".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-test",
        )
        .expect("create key profile");

        let updated = update_key_profile_record(
            &connection,
            &UpdateKeyProfileInput {
                id: profile.id,
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: None,
            },
            None,
        )
        .expect("update key profile");

        assert_eq!(updated.provider, "yuchat");
        assert_eq!(updated.nickname, "语聊");
        assert_eq!(updated.masked_secret, Some("exam...cdef".to_string()));
        assert_eq!(
            updated.secret_ref,
            Some("keychain://key-profile-test".to_string())
        );
    }

    #[test]
    fn activating_credential_profile_marks_only_one_profile_active() {
        let connection = setup_test_connection();
        sync_account_credential_profiles(&connection).expect("sync official profiles");
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "备用 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-test",
        )
        .expect("create key profile");

        let active = activate_credential_profile_record(&connection, key_profile.id)
            .expect("activate profile");

        assert!(active.is_active);
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
            .expect("count active profiles");
        assert_eq!(active_count, 1);
        let active_account_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
            .expect("count active accounts");
        assert_eq!(active_account_count, 0);
    }

    #[test]
    fn activating_official_account_profile_deactivates_key_profile() {
        let connection = setup_test_connection();
        sync_account_credential_profiles(&connection).expect("sync official profiles");
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "备用 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-test",
        )
        .expect("create key profile");

        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");
        let active = activate_account_credential_profile_record(&connection, 1)
            .expect("activate official account profile");

        assert_eq!(active.profile_kind, "official_account");
        assert_eq!(active.linked_account_id, Some(1));
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
            .expect("count active profiles");
        assert_eq!(active_count, 1);
        let key_active: i64 = connection
            .query_row(
                "SELECT is_active FROM credential_profiles WHERE id = ?1",
                [key_profile.id],
                |row| row.get(0),
            )
            .expect("query key active");
        assert_eq!(key_active, 0);
        let active_account_id: i64 = connection
            .query_row("SELECT id FROM accounts WHERE is_active = 1", [], |row| {
                row.get(0)
            })
            .expect("query active account");
        assert_eq!(active_account_id, 1);
    }

    #[test]
    fn deleting_inactive_third_party_key_profile_removes_only_that_profile() {
        let connection = setup_test_connection();
        sync_account_credential_profiles(&connection).expect("sync official profiles");
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "可删除 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-delete-test",
        )
        .expect("create key profile");

        delete_credential_profile_record(&connection, key_profile.id).expect("delete inactive key");

        let key_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE id = ?1",
                [key_profile.id],
                |row| row.get(0),
            )
            .expect("count deleted key");
        assert_eq!(key_count, 0);

        let official_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles WHERE profile_kind = 'official_account'",
                [],
                |row| row.get(0),
            )
            .expect("count official profiles");
        assert_eq!(official_count, 3);
    }

    #[test]
    fn deleting_active_third_party_key_profile_is_blocked() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "当前 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-active-delete-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let reason = delete_credential_profile_record(&connection, key_profile.id)
            .expect_err("active key should not be deleted");
        assert!(reason.contains("当前登录的 Key"));
    }

    #[test]
    fn deleting_current_official_account_is_blocked() {
        let connection = setup_test_connection();
        let current_login = CurrentCodexLogin {
            logged_in: true,
            email: Some("demo@example.com".to_string()),
            account_id: Some("profile-1".to_string()),
            is_bound: true,
        };
        connection
            .execute(
                "UPDATE accounts
                 SET is_active = 0,
                     profile_ref = ?1,
                     account_email = ?2
                 WHERE id = 2",
                params!["profile-1", "demo@example.com"],
            )
            .expect("mark account as current login");

        let reason = delete_account_record(&connection, 2, Some(&current_login))
            .expect_err("current official account should not be deleted");
        assert!(reason.contains("当前登录的官方账号"));
    }

    #[test]
    fn deleting_inactive_official_account_removes_mirrored_profile() {
        let connection = setup_test_connection();
        sync_account_credential_profiles(&connection).expect("sync official profiles");

        delete_account_record(&connection, 2, None).expect("delete inactive official account");

        let account_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM accounts WHERE id = 2", [], |row| {
                row.get(0)
            })
            .expect("count deleted account");
        assert_eq!(account_count, 0);

        let profile_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM credential_profiles
                 WHERE profile_kind = 'official_account' AND linked_account_id = 2",
                [],
                |row| row.get(0),
            )
            .expect("count deleted official profile");
        assert_eq!(profile_count, 0);
    }

    #[test]
    fn ensure_one_active_does_not_restore_account_when_key_is_active() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "备用 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-test",
        )
        .expect("create key profile");

        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");
        ensure_one_active(&connection).expect("ensure active");

        let active_account_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
            .expect("count active accounts");
        assert_eq!(active_account_count, 0);
    }

    #[test]
    fn tray_presentation_shows_active_key_identity() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "custom".to_string(),
                nickname: "备用 Key".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://key-profile-test",
        )
        .expect("create key profile");

        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let presentation = current_tray_presentation(&connection);

        assert_eq!(presentation.title, "Key 备用 Key");
        assert!(presentation.tooltip.contains("custom"));
        assert!(presentation.tooltip.contains("exam...cdef"));
        assert!(presentation.detail.contains("当前：Key"));
        assert_eq!(presentation.sampling, "Key 身份无需采样");
        assert!(!presentation.can_sample);
    }

    #[test]
    fn key_profile_runtime_files_write_auth_and_named_provider_config() {
        let config_dir = unique_temp_file("key-runtime-config");
        let _ = fs::remove_dir_all(&config_dir);
        fs::create_dir_all(&config_dir).expect("create config dir");

        let profile = CredentialProfile {
            id: 1,
            profile_kind: "third_party_key".to_string(),
            provider: "yuchat".to_string(),
            nickname: "YuChat".to_string(),
            status: "unknown".to_string(),
            is_active: true,
            base_url: Some("https://sub2api.yuchat.top".to_string()),
            model: Some("gpt-5-codex".to_string()),
            masked_secret: Some("exam...cdef".to_string()),
            secret_ref: Some("keychain://key-profile-test".to_string()),
            linked_account_id: None,
            usage_provider_type: None,
            usage_query_user: None,
            usage_query_app_version: None,
            usage_masked_secret: None,
            usage_secret_ref: None,
            usage_summary: None,
        };

        write_key_profile_runtime_files(&config_dir, &profile, "example-api-key-1234567890abcdef")
            .expect("write key runtime files");

        let auth_json = fs::read_to_string(config_dir.join("auth.json")).expect("read auth");
        let config_toml = fs::read_to_string(config_dir.join("config.toml")).expect("read config");

        assert!(auth_json.contains("\"OPENAI_API_KEY\":\"example-api-key-1234567890abcdef\""));
        assert!(config_toml.contains("model_provider = \"codexswitcher-key-1\""));
        assert!(config_toml.contains("[model_providers.\"codexswitcher-key-1\"]"));
        assert!(config_toml.contains("name = \"yuchat\""));
        assert!(config_toml.contains("model = \"gpt-5-codex\""));
        assert!(config_toml.contains("base_url = \"https://sub2api.yuchat.top/v1\""));
        assert!(config_toml.contains("requires_openai_auth = true"));

        let _ = fs::remove_dir_all(config_dir);
    }

    #[test]
    fn official_runtime_files_clear_custom_key_provider_config() {
        let config_dir = unique_temp_file("official-runtime-config");
        let _ = fs::remove_dir_all(&config_dir);
        fs::create_dir_all(&config_dir).expect("create config dir");

        let key_profile = CredentialProfile {
            id: 1,
            profile_kind: "third_party_key".to_string(),
            provider: "custom".to_string(),
            nickname: "YuChat".to_string(),
            status: "unknown".to_string(),
            is_active: true,
            base_url: Some("https://sub2api.yuchat.top".to_string()),
            model: Some("gpt-5-codex".to_string()),
            masked_secret: Some("exam...cdef".to_string()),
            secret_ref: Some("keychain://key-profile-test".to_string()),
            linked_account_id: None,
            usage_provider_type: None,
            usage_query_user: None,
            usage_query_app_version: None,
            usage_masked_secret: None,
            usage_secret_ref: None,
            usage_summary: None,
        };
        write_key_profile_runtime_files(&config_dir, &key_profile, "sk-test")
            .expect("write key runtime files");

        write_official_account_runtime_files(
            &config_dir,
            "{\"auth_mode\":\"chatgpt\",\"tokens\":{\"account_id\":\"acct_123\"}}",
        )
        .expect("write official runtime files");

        let auth_json = fs::read_to_string(config_dir.join("auth.json")).expect("read auth");
        let config_toml = fs::read_to_string(config_dir.join("config.toml")).expect("read config");

        assert!(auth_json.contains("\"auth_mode\":\"chatgpt\""));
        assert!(!auth_json.contains("OPENAI_API_KEY"));
        assert!(config_toml.contains("model = \"gpt-5-codex\""));
        assert!(config_toml.contains("model_reasoning_effort = \"high\""));
        assert!(!config_toml.contains("model_provider = \"custom\""));
        assert!(!config_toml.contains("[model_providers.custom]"));
        assert!(!config_toml.contains("base_url"));
        assert!(!config_toml.contains("requires_openai_auth"));

        let _ = fs::remove_dir_all(config_dir);
    }

    #[test]
    fn usage_risk_uses_lowest_remaining_percent() {
        assert_eq!(
            usage_risk_from_windows(0, 100).as_str(),
            AccountStatus::Exhausted.as_str()
        );
        assert_eq!(
            usage_risk_from_windows(16, 45).as_str(),
            AccountStatus::Healthy.as_str()
        );
        assert_eq!(
            usage_risk_from_windows(15, 45).as_str(),
            AccountStatus::Warning.as_str()
        );
        assert_eq!(
            usage_risk_from_windows(40, 50).as_str(),
            AccountStatus::Healthy.as_str()
        );
    }

    #[test]
    fn account_detail_includes_recent_records_and_keychain_state() {
        let connection = setup_test_connection();
        insert_account_notification(
            &connection,
            1,
            "warning",
            "测试预警",
            "账号进入测试预警状态",
            "system",
            "test_warning",
            None,
        )
        .expect("insert account notification");

        let detail = build_account_detail(&connection, 1).expect("build account detail");
        assert_eq!(detail.account.id, 1);
        assert!(!detail.recent_snapshots.is_empty());
        assert!(!detail.recent_sessions.is_empty());
        assert!(!detail.recent_notifications.is_empty());
        assert!(detail.keychain_readable);
        assert!(detail.bound_snapshot_summary.is_some());
        assert!(detail.last_failure_reason.is_some());
        assert!(detail.diagnostic_text.contains("账号 A"));
    }

    #[test]
    fn account_detail_survives_missing_keychain_or_snapshot() {
        let connection = setup_test_connection();
        connection
            .execute(
                "UPDATE accounts SET session_ref = 'keychain://missing-detail-test' WHERE id = 2",
                [],
            )
            .expect("mark missing keychain");

        let detail = build_account_detail(&connection, 2).expect("build account detail");
        assert_eq!(detail.account.id, 2);
        assert!(detail.diagnostic_text.contains("账号 B"));
        assert!(detail.recent_snapshots.len() <= 6);
    }

    #[test]
    fn startup_health_reports_database_and_tables() {
        let connection = setup_test_connection();
        let health = build_startup_health(&connection, &std::env::temp_dir(), now_text());

        assert!(health.healthy);
        assert!(health
            .checks
            .iter()
            .any(|check| check.label == "SQLite quick_check" && check.ok));
        assert!(health
            .checks
            .iter()
            .any(|check| check.label == "数据表 accounts" && check.ok));
        assert!(health
            .checks
            .iter()
            .any(|check| check.label == "基础设置记录" && check.ok));
    }

    #[test]
    fn recovery_notifications_are_inserted_once() {
        let connection = setup_test_connection();
        let reset_at = (Local::now() + Duration::minutes(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (1, ?1, 100, 20, 'exhausted', ?2, NULL, 'real_usage', '精确', 0, '{\"kind\":\"test\"}')",
                params![now_text(), reset_at],
            )
            .expect("insert exhausted snapshot");

        maybe_enqueue_recovery_notifications(&connection).expect("insert recovery notification");
        maybe_enqueue_recovery_notifications(&connection).expect("dedupe recovery notification");

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM notifications WHERE action_type = 'recovery_soon_5h'",
                [],
                |row| row.get(0),
            )
            .expect("count recovery notifications");

        assert_eq!(count, 1);
    }

    #[test]
    fn structured_notification_records_account_and_action_type() {
        let connection = setup_test_connection();
        insert_account_notification(
            &connection,
            2,
            "success",
            "切换后自动采样完成",
            "已切换到 2027，并刷新真实用量。",
            "real_switch",
            "switch_sampled",
            Some(7),
        )
        .expect("insert structured notification");

        let item = query_notifications(&connection)
            .expect("query notifications")
            .into_iter()
            .find(|item| item.title == "切换后自动采样完成")
            .expect("structured notification");

        assert_eq!(item.account_id, Some(2));
        assert_eq!(item.action_type, "switch_sampled");
        assert_eq!(item.related_handoff_id, Some(7));
    }

    #[test]
    fn cleanup_preview_and_result_remove_only_debug_records() {
        let connection = setup_test_connection();
        connection
            .execute(
                "INSERT INTO handoff_cards (account_id, task_title, goal, done_summary, todo_summary, changed_files, recent_commands, suggested_prompt, created_at)
                 VALUES (1, '切换前自动接力', '', '', '', '', '', '', ?1)",
                [now_text()],
            )
            .expect("insert old handoff");
        insert_notification(
            &connection,
            "info",
            "Gmail 真实额度暂不可读",
            "当前已完成真实登录态校验，但还没有稳定真实额度读取链路。",
            "real_verification",
        )
        .expect("insert old notification");
        insert_account_notification(
            &connection,
            1,
            "success",
            "保留的新通知",
            "账号关联通知应保留。",
            "real_switch",
            "switch_success",
            None,
        )
        .expect("insert structured notification");

        let preview = cleanup_preview(&connection).expect("preview cleanup");
        assert!(preview.old_handoff_count >= 1);
        assert!(preview.old_notification_count >= 1);

        let result = cleanup_historical_debug_data(&connection).expect("cleanup debug data");
        assert!(result.deleted_total >= 2);

        let remaining_old_handoffs: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM handoff_cards WHERE task_title = '切换前自动接力'",
                [],
                |row| row.get(0),
            )
            .expect("count old handoffs");
        let structured_notifications: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM notifications WHERE action_type = 'switch_success'",
                [],
                |row| row.get(0),
            )
            .expect("count structured notifications");

        assert_eq!(remaining_old_handoffs, 0);
        assert_eq!(structured_notifications, 1);
    }

    #[test]
    fn rollback_restores_previous_active_account() {
        let connection = setup_test_connection();
        let previous_id = active_account_id(&connection).expect("previous active");

        set_active_account(&connection, 2).expect("switch to target");
        rollback_active_account(&connection, Some(previous_id), ensure_one_active)
            .expect("rollback");

        assert_eq!(active_account_id(&connection), Some(previous_id));
    }

    #[test]
    fn failed_switch_is_logged_and_preserves_active_account() {
        let connection = setup_test_connection();
        let previous_id = active_account_id(&connection).expect("previous active");

        connection
            .execute(
                "UPDATE accounts SET status = 'exhausted', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark exhausted");

        let target = query_account_by_id(&connection, 2).expect("target account");
        let reason = is_switchable(&target).expect_err("should block switching");
        insert_switch_log(&connection, Some(previous_id), 2, "failed", &reason)
            .expect("insert failed log");

        assert_eq!(active_account_id(&connection), Some(previous_id));

        let latest_logs = query_switch_logs(&connection).expect("query logs");
        assert_eq!(latest_logs[0].result, "failed");
    }

    #[test]
    fn handoff_and_notifications_seeded() {
        let connection = setup_test_connection();
        let legacy_handoffs: i64 = connection
            .query_row("SELECT COUNT(*) FROM handoff_cards", [], |row| row.get(0))
            .expect("count legacy handoffs");
        assert_eq!(legacy_handoffs, 0);
        assert!(!query_notifications(&connection)
            .expect("notifications")
            .is_empty());
    }

    #[test]
    fn legacy_handoffs_are_purged_without_session_migration() {
        let connection = setup_test_connection();
        let now = now_text();
        connection
            .execute(
                "INSERT INTO handoff_cards (account_id, task_title, goal, created_at)
                 VALUES (1, '测试接力', '不应迁移', ?1)",
                [&now],
            )
            .expect("insert legacy handoff");
        connection
            .execute(
                "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES ('历史接力卡', 'legacy://handoff-cards', NULL, ?1, ?1, ?1)",
                [&now],
            )
            .expect("insert legacy project");
        let legacy_project_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_records (project_id, owner_account_id, owner_profile_kind, owner_profile_ref, record_type, title, summary, raw_content, message_count, source_record_id, created_at, updated_at)
                 VALUES (?1, 1, 'official_account', 'account:1', 'legacy_handoff', '测试接力', '旧接力内容', '{}', 0, NULL, ?2, ?2)",
                params![legacy_project_id, now],
            )
            .expect("insert legacy session");
        let legacy_session_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_profile_links (session_id, profile_kind, profile_ref, access_mode, source_session_id, created_at)
                 VALUES (?1, 'official_account', 'account:1', 'owner', NULL, ?2)",
                params![legacy_session_id, now_text()],
            )
            .expect("insert legacy session link");
        connection
            .execute(
                "INSERT INTO notifications (account_id, level, title, message, source_type, action_type, related_handoff_id, created_at)
                 VALUES (1, 'info', '旧接力', '不应保留', 'handoff', 'create_handoff', 1, ?1)",
                [now_text()],
            )
            .expect("insert legacy notification");

        purge_legacy_handoff_cards(&connection).expect("purge legacy handoff cards");

        for (label, sql) in [
            ("handoff_cards", "SELECT COUNT(*) FROM handoff_cards"),
            (
                "legacy sessions",
                "SELECT COUNT(*) FROM session_records WHERE record_type = 'legacy_handoff'",
            ),
            (
                "legacy links",
                "SELECT COUNT(*) FROM session_profile_links WHERE session_id = ?1",
            ),
            (
                "legacy projects",
                "SELECT COUNT(*) FROM local_projects WHERE workspace_path = 'legacy://handoff-cards'",
            ),
            (
                "legacy notifications",
                "SELECT COUNT(*) FROM notifications WHERE source_type = 'handoff' OR action_type IN ('create_handoff', 'switch_handoff_created')",
            ),
        ] {
            let count: i64 = if sql.contains("?1") {
                connection
                    .query_row(sql, [legacy_session_id], |row| row.get(0))
                    .expect(label)
            } else {
                connection.query_row(sql, [], |row| row.get(0)).expect(label)
            };
            assert_eq!(count, 0, "{label} should be purged");
        }
    }

    #[test]
    fn workspace_support_data_wraps_projects_sessions_and_notifications() {
        let connection = setup_test_connection();
        let support_data =
            query_workspace_support_data(&connection).expect("workspace support data");

        assert!(!support_data.projects.is_empty());
        assert!(!support_data.sessions.is_empty());
        assert!(!support_data.notifications.is_empty());
    }

    #[test]
    fn import_codex_local_sessions_creates_projects_and_sessions_once() {
        let connection = setup_test_connection();
        let root = std::env::temp_dir().join(format!(
            "codexswitcher-import-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let session_dir = root.join("sessions/2026/04/25");
        fs::create_dir_all(&session_dir).expect("create test session dir");
        fs::write(
            root.join("session_index.jsonl"),
            r#"{"id":"session-a","thread_name":"导入器测试会话","updated_at":"2026-04-25T06:01:02Z"}"#,
        )
        .expect("write session index");
        fs::write(
            session_dir.join("rollout-2026-04-25T14-00-00-session-a.jsonl"),
            r#"{"timestamp":"2026-04-25T06:00:00Z","type":"session_meta","payload":{"id":"session-a","timestamp":"2026-04-25T06:00:00Z","cwd":"/Users/admin/IdeaProjects/Code-Island"}}
{"timestamp":"2026-04-25T06:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}
{"timestamp":"2026-04-25T06:00:20Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"world"}]}}
"#,
        )
        .expect("write session file");

        let first = import_codex_local_sessions_from_dir(&connection, &root).expect("first import");
        let second =
            import_codex_local_sessions_from_dir(&connection, &root).expect("second import");

        assert_eq!(first.scanned_files, 1);
        assert_eq!(first.imported_sessions, 1);
        assert_eq!(first.updated_sessions, 0);
        assert_eq!(second.imported_sessions, 0);
        assert_eq!(second.updated_sessions, 1);

        let project_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM local_projects WHERE workspace_path = '/Users/admin/IdeaProjects/Code-Island'",
                [],
                |row| row.get(0),
            )
            .expect("count imported project");
        assert_eq!(project_count, 1);

        let imported: (String, i64, Option<i64>, String, String) = connection
            .query_row(
                "SELECT title, message_count, owner_account_id, owner_profile_kind, owner_profile_ref FROM session_records
                 WHERE record_type = 'codex_imported' AND external_session_id = 'session-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("query imported session");
        assert_eq!(imported.0, "导入器测试会话");
        assert_eq!(imported.1, 2);
        assert_eq!(imported.2, Some(1));
        assert_eq!(imported.3, "official_account");
        assert_eq!(imported.4, "account:1");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_codex_local_sessions_uses_user_prompt_title_when_index_is_unnamed() {
        let connection = setup_test_connection();
        let root = std::env::temp_dir().join(format!(
            "codexswitcher-import-title-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let session_dir = root.join("sessions/2026/04/26");
        fs::create_dir_all(&session_dir).expect("create test session dir");
        fs::write(
            root.join("session_index.jsonl"),
            r#"{"id":"session-title-fallback","thread_name":"未命名 Codex 会话","updated_at":"2026-04-26T06:01:02Z"}"#,
        )
        .expect("write session index");
        fs::write(
            session_dir.join("rollout-2026-04-26T14-00-00-session-title-fallback.jsonl"),
            r#"{"timestamp":"2026-04-26T06:00:00Z","type":"session_meta","payload":{"id":"session-title-fallback","timestamp":"2026-04-26T06:00:00Z","cwd":"/Users/admin/IdeaProjects/Visora"}}
{"timestamp":"2026-04-26T06:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"优化项目记录库样式并默认收起"}]}}
{"timestamp":"2026-04-26T06:00:20Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"好的"}]}}
"#,
        )
        .expect("write session file");

        import_codex_local_sessions_from_dir(&connection, &root).expect("import sessions");

        let title: String = connection
            .query_row(
                "SELECT title FROM session_records
                 WHERE record_type = 'codex_imported' AND external_session_id = 'session-title-fallback'",
                [],
                |row| row.get(0),
            )
            .expect("query imported session title");
        assert_eq!(title, "优化项目记录库样式并默认收起");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn listing_codex_imported_sessions_does_not_reassign_existing_owner() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://active-key-owner-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let now = now_text();
        connection
            .execute(
                "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES ('Visora', '/Users/admin/IdeaProjects/Visora', NULL, ?1, ?1, ?1)",
                [&now],
            )
            .expect("insert project");
        let project_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_records
                    (project_id, owner_account_id, owner_profile_kind, owner_profile_ref,
                     record_type, title, summary, raw_content, message_count,
                     source_record_id, external_session_id, created_at, updated_at)
                 VALUES (?1, NULL, 'local_codex', 'local', 'codex_imported',
                         '未命名 Codex 会话', '旧导入', '{}', 3, NULL, 'legacy-local', ?2, ?2)",
                params![project_id, now],
            )
            .expect("insert unbound session");

        let records = query_session_records(&connection).expect("query sessions");
        let record = records
            .iter()
            .find(|item| item.record_type == "codex_imported")
            .expect("imported session");
        assert_eq!(record.owner_account_id, None);
        assert_eq!(record.owner_profile_kind, "local_codex");
        assert_eq!(record.owner_profile_ref, "local");
        assert_ne!(record.owner_profile_ref, format!("key:{}", key_profile.id));

        let link_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM session_profile_links
                 WHERE session_id = ?1 AND profile_kind = 'local_codex' AND profile_ref = 'local'",
                [record.id],
                |row| row.get(0),
            )
            .expect("count existing owner link");
        assert_eq!(link_count, 0);

        let key_link_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM session_profile_links
                 WHERE session_id = ?1 AND profile_kind = 'third_party_key'",
                [record.id],
                |row| row.get(0),
            )
            .expect("count key owner links");
        assert_eq!(key_link_count, 0);
    }

    #[test]
    fn selected_codex_local_session_import_uses_active_key_owner() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://selected-import-key-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-selected-import-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let session_dir = root.join("sessions/2026/04/26");
        fs::create_dir_all(&session_dir).expect("create test session dir");
        fs::write(
            root.join("session_index.jsonl"),
            r#"{"id":"key-owned-session","thread_name":"Key 下当前项目会话","updated_at":"2026-04-26T04:01:02Z"}"#,
        )
        .expect("write session index");
        fs::write(
            session_dir.join("rollout-2026-04-26T12-00-00-key-owned-session.jsonl"),
            r#"{"timestamp":"2026-04-26T04:00:00Z","type":"session_meta","payload":{"id":"key-owned-session","timestamp":"2026-04-26T04:00:00Z","cwd":"/Users/admin/IdeaProjects/CodexSwitcher"}}
{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello from key"}]}}
"#,
        )
        .expect("write session file");

        let result = import_codex_local_session_candidates_from_dir(
            &connection,
            &root,
            &["key-owned-session".to_string()],
            true,
        )
        .expect("import selected candidate");

        assert_eq!(result.imported_sessions, 1);
        let imported: (Option<i64>, String, String, String) = connection
            .query_row(
                "SELECT owner_account_id, owner_profile_kind, owner_profile_ref, title
                 FROM session_records
                 WHERE record_type = 'codex_imported' AND external_session_id = 'key-owned-session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("query selected import");

        assert_eq!(imported.0, None);
        assert_eq!(imported.1, "third_party_key");
        assert_eq!(imported.2, format!("key:{}", key_profile.id));
        assert_eq!(imported.3, "Key 下当前项目会话");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn importing_selected_candidate_registers_thread_in_codex_state() {
        let connection = setup_test_connection();
        let root = std::env::temp_dir().join(format!(
            "codexswitcher-state-register-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-register-thread.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:00Z","type":"session_meta","payload":{"id":"register-thread","timestamp":"2026-04-26T04:00:00Z","cwd":"/Users/admin/IdeaProjects/CodexSwitcher"}}
{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"同步到 Codex 侧边栏"}]}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode)
                 VALUES
                    ('register-thread', ?1, 1777176000, 1777176060, 'codex-cli', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher', '同步到 Codex',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let result = import_codex_local_session_candidates_from_dir(
            &connection,
            &root,
            &["register-thread".to_string()],
            true,
        )
        .expect("import selected candidate");

        assert_eq!(result.imported_sessions, 1);
        assert_eq!(result.codex_synced_threads, 1);
        assert_eq!(result.codex_skipped_threads, 0);

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let synced: (
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
            i64,
            i64,
        ) = state
            .query_row(
                "SELECT title, cwd, archived, first_user_message, model_provider,
                        thread_source, preview, recency_at, recency_at_ms
                 FROM threads WHERE id = 'register-thread'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .expect("query synced thread");
        assert_eq!(synced.0, "同步到 Codex");
        assert_eq!(synced.1, "/Users/admin/IdeaProjects/CodexSwitcher");
        assert_eq!(synced.2, 0);
        assert_eq!(synced.3, "同步到 Codex 侧边栏");
        assert_eq!(synced.4, "openai");
        assert_eq!(synced.5, "user");
        assert_eq!(synced.6, "同步到 Codex");
        assert_eq!(synced.7, 1777176060);
        assert_eq!(synced.8, 1777176060000);
        let rollout_content = fs::read_to_string(&rollout_path).expect("read rollout after sync");
        let first_line = rollout_content.lines().next().expect("rollout meta line");
        let meta: Value = serde_json::from_str(first_line).expect("parse rollout meta");
        assert_eq!(
            meta.get("payload")
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str),
            Some("openai")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn importing_official_thread_into_active_key_writes_key_scoped_provider() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "oneTop".to_string(),
                nickname: "oneTop".to_string(),
                base_url: "https://www.onetopai.asia".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://official-to-key-writeback-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-official-to-key-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-official-to-key.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            root.join("config.toml"),
            "model_provider = \"renamed-provider\"\nmodel = \"gpt-5-codex\"\n",
        )
        .expect("write codex config");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:00Z","type":"session_meta","payload":{"id":"official-to-key-thread","timestamp":"2026-04-26T04:00:00Z","cwd":"/Users/admin/IdeaProjects/Visora","model_provider":"openai"}}
{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"官方账号线程导入到 Key"}]}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode)
                 VALUES
                    ('official-to-key-thread', ?1, 1777176000, 1777176060, 'vscode', 'openai',
                     '/Users/admin/IdeaProjects/Visora', '官方账号线程',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let result = import_codex_local_session_candidates_from_dir(
            &connection,
            &root,
            &["official-to-key-thread".to_string()],
            true,
        )
        .expect("import selected candidate");

        assert_eq!(result.imported_sessions, 1);
        assert_eq!(result.codex_synced_threads, 1);

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let model_provider: String = state
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 'official-to-key-thread'",
                [],
                |row| row.get(0),
            )
            .expect("query model provider");
        assert_eq!(
            model_provider,
            format!("codexswitcher-key-{}", key_profile.id)
        );

        let rollout_content = fs::read_to_string(&rollout_path).expect("read rollout after sync");
        let first_line = rollout_content.lines().next().expect("rollout meta line");
        let meta: Value = serde_json::from_str(first_line).expect("parse rollout meta");
        assert_eq!(
            meta.get("payload")
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str),
            Some(format!("codexswitcher-key-{}", key_profile.id).as_str())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn active_identity_visibility_hides_threads_owned_by_other_keys() {
        let connection = setup_test_connection();
        let first_key = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "taomu".to_string(),
                nickname: "桃木 Key A".to_string(),
                base_url: "https://api.example.com/v1".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://visibility-key-a",
        )
        .expect("create first key");
        let second_key = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "taomu".to_string(),
                nickname: "桃木 Key B".to_string(),
                base_url: "https://api.example.com/v1".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://visibility-key-b",
        )
        .expect("create second key");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-thread-visibility-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let state = create_codex_state_db(&root);
        for (id, provider) in [
            ("key-a-thread", key_profile_model_provider(&first_key)),
            ("key-b-thread", key_profile_model_provider(&second_key)),
            ("official-thread", "openai".to_string()),
            (
                "missing-key-a-thread",
                key_profile_model_provider(&first_key),
            ),
        ] {
            let rollout_path = root.join(format!("{id}.jsonl"));
            if id != "missing-key-a-thread" {
                fs::write(&rollout_path, "{}\n").expect("write visibility rollout");
            }
            state
                .execute(
                    "INSERT INTO threads
                        (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                         sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                         cli_version, first_user_message, memory_mode)
                     VALUES (?1, ?2, 1777176000, 1777176060, 'vscode', ?3, '/tmp/project', ?1,
                             'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled')",
                    params![id, rollout_path.to_string_lossy(), provider],
                )
                .expect("insert state thread");
        }
        drop(state);

        let first_owner = SessionOwner {
            account_id: None,
            profile_kind: "third_party_key".to_string(),
            profile_ref: format!("key:{}", first_key.id),
        };
        sync_codex_thread_visibility_for_owner(&connection, &root, &first_owner)
            .expect("show first key only");

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let archived = |id: &str| {
            state
                .query_row("SELECT archived FROM threads WHERE id = ?1", [id], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("read archived state")
        };
        assert_eq!(archived("key-a-thread"), 0);
        assert_eq!(archived("key-b-thread"), 1);
        assert_eq!(archived("official-thread"), 1);
        let missing_count: i64 = state
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE id = 'missing-key-a-thread'",
                [],
                |row| row.get(0),
            )
            .expect("count missing rollout thread");
        assert_eq!(missing_count, 0);
        drop(state);

        let candidates = list_codex_local_session_candidates_from_dir(&connection, &root)
            .expect("list visible and isolation-archived sessions");
        assert_eq!(candidates.len(), 3);
        for (thread_id, key) in [("key-a-thread", &first_key), ("key-b-thread", &second_key)] {
            let candidate = candidates
                .iter()
                .find(|item| item.candidate_id == thread_id)
                .expect("both Keys remain available in the manager");
            assert_eq!(candidate.identity_key, format!("key:{}", key.id));
            assert_eq!(candidate.identity_label, key.nickname);
        }

        let official_owner = SessionOwner {
            account_id: Some(1),
            profile_kind: "official_account".to_string(),
            profile_ref: "account:1".to_string(),
        };
        sync_codex_thread_visibility_for_owner(&connection, &root, &official_owner)
            .expect("show official threads only");
        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let key_a_archived: i64 = state
            .query_row(
                "SELECT archived FROM threads WHERE id = 'key-a-thread'",
                [],
                |row| row.get(0),
            )
            .expect("read key archived state");
        let official_archived: i64 = state
            .query_row(
                "SELECT archived FROM threads WHERE id = 'official-thread'",
                [],
                |row| row.get(0),
            )
            .expect("read official archived state");
        assert_eq!(key_a_archived, 1);
        assert_eq!(official_archived, 0);
        assert_eq!(
            list_codex_local_session_candidates_from_dir(&connection, &root)
                .expect("list candidates after official switch")
                .len(),
            3
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn active_identity_visibility_keeps_user_archived_thread_archived() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "taomu".to_string(),
                nickname: "桃木 Key".to_string(),
                base_url: "https://api.example.com/v1".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://user-archived-visibility-key",
        )
        .expect("create key profile");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-user-archived-visibility-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let state = create_codex_state_db(&root);
        let archived_dir = root.join("archived_sessions");
        fs::create_dir_all(&archived_dir).expect("create archived sessions dir");
        let rollout_path =
            archived_dir.join("rollout-2026-08-28T10-00-00-user-archived-thread.jsonl");
        fs::write(&rollout_path, "{}\n").expect("write user archived rollout");
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     archived_at, cli_version, first_user_message, memory_mode)
                 VALUES ('user-archived-thread', ?1, 1787882400, 1787882460, 'vscode', ?2,
                         '/tmp/project', '用户主动归档', 'workspace-write', 'on-request', 0, 1, 1,
                         1787882460, 'test', '', 'enabled')",
                params![
                    rollout_path.to_string_lossy(),
                    key_profile_model_provider(&key_profile)
                ],
            )
            .expect("insert user archived thread");
        drop(state);

        let owner = SessionOwner {
            account_id: None,
            profile_kind: "third_party_key".to_string(),
            profile_ref: format!("key:{}", key_profile.id),
        };
        sync_codex_thread_visibility_for_owner(&connection, &root, &owner)
            .expect("keep user archive hidden");

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let (archived, current_path): (i64, String) = state
            .query_row(
                "SELECT archived, rollout_path FROM threads WHERE id = 'user-archived-thread'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read user archived thread");
        assert_eq!(archived, 1);
        assert_eq!(current_path, rollout_path.to_string_lossy());
        assert!(rollout_path.exists());
        assert!(!is_codex_visibility_archive(
            &connection,
            "user-archived-thread",
            &key_profile_model_provider(&key_profile)
        )
        .expect("read visibility marker"));

        assert!(
            list_codex_local_session_candidates_from_dir(&connection, &root)
                .expect("list candidates without user archives")
                .is_empty()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_rollout_threads_are_removed_from_primary_and_legacy_indexes() {
        let root = std::env::temp_dir().join(format!(
            "codexswitcher-missing-rollout-index-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let existing_rollout = root.join("sessions/existing-thread.jsonl");
        fs::create_dir_all(existing_rollout.parent().expect("rollout parent"))
            .expect("create rollout dir");
        fs::write(&existing_rollout, "{}\n").expect("write existing rollout");

        for (state_root, missing_id) in [
            (root.clone(), "missing-primary-thread"),
            (root.join("sqlite"), "missing-legacy-thread"),
        ] {
            let state = create_codex_state_db(&state_root);
            for (thread_id, rollout_path) in [
                (
                    missing_id,
                    root.join(format!("sessions/{missing_id}.jsonl")),
                ),
                ("existing-thread", existing_rollout.clone()),
            ] {
                state
                    .execute(
                        "INSERT INTO threads
                            (id, rollout_path, created_at, updated_at, source, model_provider, cwd,
                             title, sandbox_policy, approval_mode, archived, cli_version,
                             first_user_message, memory_mode)
                         VALUES (?1, ?2, 1787882400, 1787882460, 'vscode', 'openai', '/tmp', ?1,
                                 'workspace-write', 'on-request', 0, 'test', '', 'enabled')",
                        params![thread_id, rollout_path.to_string_lossy()],
                    )
                    .expect("insert indexed thread");
            }
        }

        let removed =
            purge_missing_codex_thread_indexes(&root, true).expect("purge missing indexes");
        assert_eq!(removed, 2);
        for state_path in [
            root.join("state_5.sqlite"),
            root.join("sqlite/state_5.sqlite"),
        ] {
            let state = Connection::open(state_path).expect("open cleaned state");
            let ids = state
                .prepare("SELECT id FROM threads ORDER BY id")
                .expect("prepare thread ids")
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query thread ids")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect thread ids");
            assert_eq!(ids, vec!["existing-thread".to_string()]);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_thread_catalog_contains_only_visible_threads_with_rollouts() {
        let root = std::env::temp_dir().join(format!(
            "codexswitcher-local-thread-catalog-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let valid_rollout = root.join("sessions/valid-thread.jsonl");
        let archived_rollout = root.join("archived_sessions/archived-thread.jsonl");
        fs::create_dir_all(valid_rollout.parent().expect("valid rollout parent"))
            .expect("create sessions dir");
        fs::create_dir_all(archived_rollout.parent().expect("archived rollout parent"))
            .expect("create archived sessions dir");
        fs::write(&valid_rollout, "{}\n").expect("write valid rollout");
        fs::write(&archived_rollout, "{}\n").expect("write archived rollout");

        let state = create_codex_state_db(&root);
        for (thread_id, title, rollout_path, archived) in [
            ("valid-thread", "有效会话", valid_rollout.clone(), 0),
            ("archived-thread", "已归档会话", archived_rollout.clone(), 1),
            (
                "missing-thread",
                "已删除会话",
                root.join("sessions/missing-thread.jsonl"),
                0,
            ),
        ] {
            state
                .execute(
                    "INSERT INTO threads
                        (id, rollout_path, created_at, updated_at, source, model_provider, cwd,
                         title, sandbox_policy, approval_mode, archived, archived_at, cli_version,
                         first_user_message, memory_mode, recency_at)
                     VALUES (?1, ?2, 1787882400, 1787882460, 'vscode', 'codexswitcher-key-9',
                             '/tmp/project', ?3, 'workspace-write', 'on-request', ?4,
                             CASE WHEN ?4 = 1 THEN 1787882460 ELSE NULL END, 'test', '',
                             'enabled', 1787882460)",
                    params![thread_id, rollout_path.to_string_lossy(), title, archived],
                )
                .expect("insert catalog source thread");
        }
        drop(state);

        let catalog_dir = root.join("sqlite");
        fs::create_dir_all(&catalog_dir).expect("create catalog dir");
        let catalog =
            Connection::open(catalog_dir.join("codex-dev.db")).expect("open local thread catalog");
        catalog
            .execute_batch(
                "CREATE TABLE local_thread_catalog (
                    host_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    display_title TEXT NOT NULL,
                    source_created_at REAL NOT NULL,
                    source_updated_at REAL NOT NULL,
                    cwd TEXT,
                    source_kind TEXT NOT NULL,
                    source_detail TEXT,
                    model_provider TEXT,
                    git_branch TEXT,
                    observation_sequence INTEGER NOT NULL,
                    missing_candidate INTEGER NOT NULL DEFAULT 0,
                    thread_source TEXT,
                    source_recency_at REAL NOT NULL DEFAULT 0,
                    pending_observed_title INTEGER NOT NULL DEFAULT 0,
                    project_id TEXT,
                    conversation_origin TEXT,
                    PRIMARY KEY (host_id, thread_id)
                );
                CREATE TABLE local_thread_catalog_metadata (
                    id INTEGER PRIMARY KEY,
                    catalog_revision INTEGER NOT NULL
                );
                INSERT INTO local_thread_catalog_metadata (id, catalog_revision) VALUES (1, 7);
                INSERT INTO local_thread_catalog
                    (host_id, thread_id, display_title, source_created_at, source_updated_at,
                     source_kind, observation_sequence, missing_candidate, source_recency_at,
                     pending_observed_title)
                VALUES
                    ('local', 'stale-thread', '幽灵会话', 1, 1, 'vscode', 3, 0, 1, 0),
                    ('local', 'archived-thread', '已归档会话', 1, 1, 'vscode', 3, 0, 1, 0),
                    ('local', 'cli-thread', '命令行会话', 1, 1, 'cli', 3, 0, 1, 0);",
            )
            .expect("seed local thread catalog");
        drop(catalog);

        let removed = sync_codex_local_thread_catalog(&root).expect("sync local thread catalog");
        assert_eq!(removed, 2);

        let catalog = Connection::open(catalog_dir.join("codex-dev.db"))
            .expect("reopen local thread catalog");
        let vscode_rows = catalog
            .prepare(
                "SELECT thread_id, display_title FROM local_thread_catalog
                 WHERE host_id = 'local' AND source_kind = 'vscode' ORDER BY thread_id",
            )
            .expect("prepare visible catalog rows")
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query visible catalog rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect visible catalog rows");
        assert_eq!(
            vscode_rows,
            vec![("valid-thread".to_string(), "有效会话".to_string())]
        );
        let cli_count: i64 = catalog
            .query_row(
                "SELECT COUNT(*) FROM local_thread_catalog
                 WHERE host_id = 'local' AND thread_id = 'cli-thread'",
                [],
                |row| row.get(0),
            )
            .expect("count preserved cli catalog row");
        assert_eq!(cli_count, 1);
        let revision: i64 = catalog
            .query_row(
                "SELECT catalog_revision FROM local_thread_catalog_metadata WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("read catalog revision");
        assert_eq!(revision, 8);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn listing_records_repairs_existing_key_import_thread_provider() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "oneTop".to_string(),
                nickname: "oneTop".to_string(),
                base_url: "https://www.onetopai.asia".to_string(),
                model: "gpt-5-codex".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://existing-key-writeback-test",
        )
        .expect("create key profile");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-existing-key-repair-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-existing-key.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:00Z","type":"session_meta","payload":{"id":"existing-key-thread","timestamp":"2026-04-26T04:00:00Z","cwd":"/Users/admin/IdeaProjects/Visora","model_provider":"openai"}}
{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"已有 Key 导入记录"}]}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode)
                 VALUES
                    ('existing-key-thread', ?1, 1777176000, 1777176060, 'vscode', 'openai',
                     '/Users/admin/IdeaProjects/Visora', '已有 Key 导入记录',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let now = now_text();
        connection
            .execute(
                "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES ('Visora', '/Users/admin/IdeaProjects/Visora', NULL, ?1, ?1, ?1)",
                [&now],
            )
            .expect("insert local project");
        let project_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_records
                    (project_id, owner_account_id, owner_profile_kind, owner_profile_ref,
                     record_type, title, summary, raw_content, message_count,
                     source_record_id, external_session_id, created_at, updated_at)
                 VALUES (?1, NULL, 'third_party_key', ?2, 'codex_imported',
                         '已有 Key 导入记录', 'summary', ?3, 1, NULL, 'existing-key-thread', ?4, ?4)",
                params![
                    project_id,
                    format!("key:{}", key_profile.id),
                    json!({
                        "source": "codex_local_session",
                        "session_id": "existing-key-thread",
                        "source_path": rollout_path.to_string_lossy().to_string(),
                        "workspace_path": "/Users/admin/IdeaProjects/Visora",
                    })
                    .to_string(),
                    now
                ],
            )
            .expect("insert imported record");

        backfill_codex_imported_state_model_providers_for_dir(&connection, &root)
            .expect("repair existing key import");

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        let model_provider: String = state
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 'existing-key-thread'",
                [],
                |row| row.get(0),
            )
            .expect("query model provider");
        assert_eq!(
            model_provider,
            format!("codexswitcher-key-{}", key_profile.id)
        );

        let rollout_content = fs::read_to_string(&rollout_path).expect("read rollout after sync");
        let first_line = rollout_content.lines().next().expect("rollout meta line");
        let meta: Value = serde_json::from_str(first_line).expect("parse rollout meta");
        assert_eq!(
            meta.get("payload")
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str),
            Some(format!("codexswitcher-key-{}", key_profile.id).as_str())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_library_preserves_history_beyond_active_index_and_first_hundred() {
        let connection = setup_test_connection();
        let owner = SessionOwner {
            account_id: None,
            profile_kind: "third_party_key".to_string(),
            profile_ref: "key:9".to_string(),
        };
        for index in 0..105 {
            let session = ParsedCodexLocalSession {
                session_id: format!("history-{index}"),
                workspace_path: "/tmp/library".to_string(),
                title: format!("历史会话 {index}"),
                created_at: now_text(),
                updated_at: now_text(),
                message_count: 1,
                source_path: "/missing/old-rollout.jsonl".to_string(),
                source: "vscode".to_string(),
                model_provider: "custom".to_string(),
            };
            upsert_codex_imported_session(&connection, &session, &owner, true)
                .expect("store history");
        }
        let records = query_session_records(&connection)
            .expect("query full library")
            .into_iter()
            .filter(|record| record.record_type == "codex_imported")
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 105);
        let non_main_ids = HashSet::from(["history-0".to_string()]);
        let retained = records
            .iter()
            .filter(|record| is_main_codex_session_record(record, Some(&non_main_ids)))
            .collect::<Vec<_>>();
        assert_eq!(retained.len(), 104);
        assert!(retained
            .iter()
            .all(|record| session_record_external_id(record).as_deref() != Some("history-0")));
    }

    #[test]
    fn listing_codex_candidates_reads_state_threads_with_codex_titles() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://state-thread-key-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-state-thread-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-state-thread.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello from state"}]}}
{"timestamp":"2026-04-26T04:00:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 401 Unauthorized: invalid api key, url: https://sub2api.yuchat.top/v1/responses"}}
{"timestamp":"2026-04-26T04:00:20Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"world"}]}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode, model, reasoning_effort)
                 VALUES
                    ('state-thread-key-owned', ?1, 1777176000, 1777176060, 'vscode', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher', '排查测模型与保存key问题',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled', 'gpt-5.5', 'high')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let candidates = list_codex_local_session_candidates_from_dir(&connection, &root)
            .expect("list candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].candidate_id, "state-thread-key-owned");
        assert_eq!(
            candidates[0].identity_key,
            format!("key:{}", key_profile.id)
        );
        assert_eq!(candidates[0].identity_label, "语聊");
        assert_eq!(candidates[0].identity_kind_label, "Key");
        assert_eq!(candidates[0].title, "排查测模型与保存key问题");
        assert_eq!(
            candidates[0].project_path,
            "/Users/admin/IdeaProjects/CodexSwitcher"
        );
        assert_eq!(candidates[0].message_count, 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn importing_selected_candidate_copies_existing_thread_to_active_key() {
        let connection = setup_test_connection();
        let yuchat_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://reassign-yuchat-test",
        )
        .expect("create yuchat profile");
        let one_top_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "oneTop".to_string(),
                nickname: "oneTop".to_string(),
                base_url: "https://www.onetopai.asia".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://reassign-onetop-test",
        )
        .expect("create onetop profile");
        activate_credential_profile_record(&connection, one_top_profile.id)
            .expect("activate onetop");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-reassign-import-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-reassign-owner.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:00Z","type":"session_meta","payload":{"id":"reassign-thread","timestamp":"2026-04-26T04:00:00Z","cwd":"/Users/admin/IdeaProjects/CodexSwitcher","model_provider":"yuchat"}}
{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello from reassigned thread"}]}}
{"timestamp":"2026-04-26T04:00:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 401 Unauthorized, url: https://sub2api.yuchat.top/v1/responses"}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode, model, reasoning_effort)
                 VALUES
                    ('reassign-thread', ?1, 1777176000, 1777176660, 'codex-cli', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher', '待迁移线程',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled', 'gpt-5.5', 'high')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let now = now_text();
        connection
            .execute(
                "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES ('CodexSwitcher', '/Users/admin/IdeaProjects/CodexSwitcher', NULL, ?1, ?1, ?1)",
                [&now],
            )
            .expect("insert local project");
        let project_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_records
                    (project_id, owner_account_id, owner_profile_kind, owner_profile_ref,
                     record_type, title, summary, raw_content, message_count,
                     source_record_id, external_session_id, created_at, updated_at)
                 VALUES (?1, NULL, 'third_party_key', ?2, 'codex_imported',
                         '待迁移线程', 'summary', ?3, 1, NULL, 'reassign-thread', ?4, ?4)",
                params![
                    project_id,
                    format!("key:{}", yuchat_profile.id),
                    json!({
                        "source": "codex_local_session",
                        "session_id": "reassign-thread",
                        "source_path": rollout_path.to_string_lossy().to_string(),
                        "workspace_path": "/Users/admin/IdeaProjects/CodexSwitcher",
                    })
                    .to_string(),
                    now
                ],
            )
            .expect("insert imported record");

        let state = Connection::open(root.join("state_5.sqlite")).expect("open state");
        state
            .execute(
                "UPDATE threads SET source = 'vscode' WHERE id = 'reassign-thread'",
                [],
            )
            .expect("make source a main thread");
        drop(state);
        mark_codex_visibility_archive(&connection, "reassign-thread", "custom")
            .expect("mark isolation archive");
        apply_codex_thread_archive_actions(
            &root,
            &[CodexThreadArchiveAction {
                thread_id: "reassign-thread".to_string(),
                method: "thread/archive",
            }],
        )
        .expect("archive other Key source");
        let candidates = list_codex_local_session_candidates_from_dir(&connection, &root)
            .expect("list archived import source");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].identity_key,
            format!("key:{}", yuchat_profile.id)
        );

        let result = import_codex_local_session_candidates_from_dir(
            &connection,
            &root,
            &["reassign-thread".to_string()],
            true,
        )
        .expect("reimport selected candidate");

        assert_eq!(result.imported_sessions, 1);
        assert_eq!(result.updated_sessions, 0);
        assert_eq!(result.codex_synced_threads, 1);

        let repeated = import_codex_local_session_candidates_from_dir(
            &connection,
            &root,
            &["reassign-thread".to_string()],
            true,
        )
        .expect("repeat selected candidate import");
        assert_eq!(repeated.imported_sessions, 0);
        assert_eq!(repeated.updated_sessions, 1);

        let original_owner_ref: String = connection
            .query_row(
                "SELECT owner_profile_ref
                 FROM session_records
                 WHERE record_type = 'codex_imported' AND external_session_id = 'reassign-thread'",
                [],
                |row| row.get(0),
            )
            .expect("query original owner");
        assert_eq!(original_owner_ref, format!("key:{}", yuchat_profile.id));

        let records = query_session_records(&connection).expect("query records after reimport");
        assert_eq!(
            records
                .iter()
                .filter(|record| record.title == "待迁移线程")
                .count(),
            2
        );
        let target_copy = records
            .iter()
            .find(|record| {
                record.title == "待迁移线程"
                    && record.owner_profile_ref == format!("key:{}", one_top_profile.id)
            })
            .expect("find copied record");
        assert_eq!(
            target_copy.owner_profile_ref,
            format!("key:{}", one_top_profile.id)
        );

        let copied_session_id: String = connection
            .query_row(
                "SELECT external_session_id FROM session_records WHERE id = ?1",
                [target_copy.id],
                |row| row.get(0),
            )
            .expect("copied external session id");
        assert_ne!(copied_session_id, "reassign-thread");
        let state = Connection::open(root.join("state_5.sqlite")).expect("open state db");
        assert_eq!(
            state
                .query_row(
                    "SELECT archived FROM threads WHERE id = 'reassign-thread'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("read source archive state"),
            1
        );
        let original_provider: String = state
            .query_row(
                "SELECT model_provider FROM threads WHERE id = 'reassign-thread'",
                [],
                |row| row.get(0),
            )
            .expect("query original provider");
        let (copied_provider, copied_rollout): (String, String) = state
            .query_row(
                "SELECT model_provider, rollout_path FROM threads WHERE id = ?1",
                [copied_session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query copied thread");
        assert_eq!(original_provider, "custom");
        assert_eq!(
            copied_provider,
            format!("codexswitcher-key-{}", one_top_profile.id)
        );
        assert_ne!(copied_rollout, rollout_path.to_string_lossy());
        let copied_rollout = PathBuf::from(copied_rollout);
        assert!(copied_rollout.starts_with(root.join("sessions")));
        assert!(!copied_rollout
            .to_string_lossy()
            .contains("codexswitcher-imported"));
        assert!(copied_rollout
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(&copied_session_id)));
        let copied_content = fs::read_to_string(copied_rollout).expect("read copied rollout");
        let copied_meta: Value =
            serde_json::from_str(copied_content.lines().next().expect("copied rollout meta"))
                .expect("parse copied rollout meta");
        assert_eq!(
            copied_meta
                .get("payload")
                .and_then(|payload| payload.get("id"))
                .and_then(Value::as_str),
            Some(copied_session_id.as_str())
        );
        assert_eq!(
            copied_meta
                .get("payload")
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str),
            Some(format!("codexswitcher-key-{}", one_top_profile.id).as_str())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn listing_codex_candidates_prefers_earliest_custom_provider_hint() {
        let connection = setup_test_connection();
        let yuchat_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://earliest-yuchat-test",
        )
        .expect("create yuchat profile");
        let one_top_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "oneTop".to_string(),
                nickname: "oneTop".to_string(),
                base_url: "https://www.onetopai.asia".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://earliest-onetop-test",
        )
        .expect("create onetop profile");
        activate_credential_profile_record(&connection, one_top_profile.id)
            .expect("activate onetop");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-provider-hint-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-provider-hint.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello from mixed custom thread"}]}}
{"timestamp":"2026-04-26T04:00:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 401 Unauthorized, url: https://sub2api.yuchat.top/v1/responses"}}
{"timestamp":"2026-04-26T04:10:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 403 Forbidden, url: https://www.onetopai.asia/v1/responses"}}
"#,
        )
        .expect("write rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode, model, reasoning_effort)
                 VALUES
                    ('mixed-custom-thread', ?1, 1777176000, 1777176660, 'vscode', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher', '跨 Key 线程',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled', 'gpt-5.5', 'high')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .expect("insert state thread");
        drop(state);

        let candidates = list_codex_local_session_candidates_from_dir(&connection, &root)
            .expect("list candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].identity_key,
            format!("key:{}", yuchat_profile.id)
        );
        assert_eq!(candidates[0].identity_label, "语聊");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn query_session_records_backfills_custom_owner_from_rollout_hint() {
        let connection = setup_test_connection();
        let yuchat_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://backfill-yuchat-test",
        )
        .expect("create yuchat profile");
        let one_top_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "oneTop".to_string(),
                nickname: "oneTop".to_string(),
                base_url: "https://www.onetopai.asia".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-abcdef1234567890".to_string(),
            },
            "keychain://backfill-onetop-test",
        )
        .expect("create onetop profile");
        activate_credential_profile_record(&connection, one_top_profile.id)
            .expect("activate onetop");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-owner-backfill-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let rollout_path = root.join("sessions/rollout-owner-backfill.jsonl");
        fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-04-26T04:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello from imported custom thread"}]}}
{"timestamp":"2026-04-26T04:00:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 401 Unauthorized, url: https://sub2api.yuchat.top/v1/responses"}}
{"timestamp":"2026-04-26T04:10:15Z","type":"event_msg","payload":{"type":"error","message":"unexpected status 403 Forbidden, url: https://www.onetopai.asia/v1/responses"}}
"#,
        )
        .expect("write rollout");

        let now = now_text();
        connection
            .execute(
                "INSERT INTO local_projects (name, workspace_path, git_remote, last_active_at, created_at, updated_at)
                 VALUES ('CodexSwitcher', '/Users/admin/IdeaProjects/CodexSwitcher', NULL, ?1, ?1, ?1)",
                [&now],
            )
            .expect("insert local project");
        let project_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_records
                    (project_id, owner_account_id, owner_profile_kind, owner_profile_ref,
                     record_type, title, summary, raw_content, message_count,
                     source_record_id, external_session_id, created_at, updated_at)
                 VALUES (?1, NULL, 'third_party_key', ?2, 'codex_imported',
                         '跨 Key 已导入线程', 'summary', ?3, 1, NULL, 'mixed-custom-thread', ?4, ?4)",
                params![
                    project_id,
                    format!("key:{}", one_top_profile.id),
                    json!({
                        "source": "codex_local_session",
                        "session_id": "mixed-custom-thread",
                        "source_path": rollout_path.to_string_lossy().to_string(),
                        "workspace_path": "/Users/admin/IdeaProjects/CodexSwitcher",
                    })
                    .to_string(),
                    now
                ],
            )
            .expect("insert imported record");

        let records = query_session_records(&connection).expect("query session records");
        let backfilled = records
            .into_iter()
            .find(|record| record.title == "跨 Key 已导入线程")
            .expect("find backfilled record");

        assert_eq!(backfilled.owner_profile_kind, "third_party_key");
        assert_eq!(
            backfilled.owner_profile_ref,
            format!("key:{}", yuchat_profile.id)
        );

        let stored_ref: String = connection
            .query_row(
                "SELECT owner_profile_ref FROM session_records WHERE title = '跨 Key 已导入线程'",
                [],
                |row| row.get(0),
            )
            .expect("query stored owner");
        assert_eq!(stored_ref, format!("key:{}", yuchat_profile.id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn listing_codex_candidates_excludes_internal_review_threads() {
        let connection = setup_test_connection();
        let key_profile = create_key_profile_record(
            &connection,
            CreateKeyProfileInput {
                provider: "yuchat".to_string(),
                nickname: "语聊".to_string(),
                base_url: "https://sub2api.yuchat.top".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: "example-api-key-1234567890abcdef".to_string(),
            },
            "keychain://internal-review-filter-test",
        )
        .expect("create key profile");
        activate_credential_profile_record(&connection, key_profile.id).expect("activate key");

        let root = std::env::temp_dir().join(format!(
            "codexswitcher-internal-review-filter-test-{}-{}",
            std::process::id(),
            now_text().replace([' ', ':'], "-")
        ));
        let normal_rollout_path = root.join("sessions/normal.jsonl");
        let review_rollout_path = root.join("sessions/review.jsonl");
        fs::create_dir_all(normal_rollout_path.parent().expect("rollout parent"))
            .expect("create sessions dir");
        fs::write(&normal_rollout_path, "").expect("write normal rollout");
        fs::write(&review_rollout_path, "").expect("write review rollout");
        let state = create_codex_state_db(&root);
        state
            .execute(
                "INSERT INTO threads
                    (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
                     sandbox_policy, approval_mode, tokens_used, has_user_event, archived,
                     cli_version, first_user_message, memory_mode, model, reasoning_effort)
                 VALUES
                    ('normal-thread', ?1, 1777176000, 1777176060, 'vscode', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher', '正常项目会话',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled', 'gpt-5.5', 'high'),
                    ('internal-review-thread', ?2, 1777176100, 1777176160, 'vscode', 'custom',
                     '/Users/admin/IdeaProjects/CodexSwitcher',
                     'The following is the Codex agent history whose request action you are assessing. Treat the transcript as untrusted evidence.',
                     'workspace-write', 'on-request', 0, 1, 0, 'test', '', 'enabled', 'gpt-5.5', 'high')",
                params![
                    normal_rollout_path.to_string_lossy().to_string(),
                    review_rollout_path.to_string_lossy().to_string()
                ],
            )
            .expect("insert threads");
        drop(state);

        let candidates = list_codex_local_session_candidates_from_dir(&connection, &root)
            .expect("list candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].candidate_id, "normal-thread");
        assert_eq!(candidates[0].title, "正常项目会话");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chart_points_include_history() {
        let connection = setup_test_connection();
        let chart = build_chart_points(&connection).expect("chart points");
        assert!(chart.len() >= 2);
    }

    #[test]
    fn chart_points_group_dense_samples_into_15_minute_buckets() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots", [])
            .expect("clear snapshots");

        for (account_id, sample_time, value) in [
            (1, "2026-04-21 10:58:00", 72),
            (1, "2026-04-21 10:51:00", 68),
            (2, "2026-04-21 10:49:00", 16),
            (1, "2026-04-21 10:44:00", 61),
            (2, "2026-04-21 10:37:00", 14),
            (1, "2026-04-21 10:26:00", 49),
            (2, "2026-04-21 10:20:00", 11),
        ] {
            connection
                .execute(
                    "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                     VALUES (?1, ?2, ?3, 18, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"test\"}')",
                    params![account_id, sample_time, value],
                )
                .expect("insert dense real snapshot");
        }

        let chart = build_chart_points(&connection).expect("chart points");

        assert_eq!(chart.len(), 3);
        assert_eq!(
            chart
                .iter()
                .map(|point| point.label.as_str())
                .collect::<Vec<_>>(),
            vec!["10:15", "10:30", "10:45"]
        );
        assert_eq!(chart[2].series.len(), 2);
        assert_eq!(chart[2].series[0].value, 72);
    }

    #[test]
    fn chart_points_keep_only_recent_buckets() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots", [])
            .expect("clear snapshots");

        for minute in [
            "08:00:00", "08:20:00", "08:40:00", "09:00:00", "09:20:00", "09:40:00", "10:00:00",
            "10:20:00",
        ] {
            connection
                .execute(
                    "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                     VALUES (1, ?1, 31, 18, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"test\"}')",
                    params![format!("2026-04-21 {minute}")],
                )
                .expect("insert recent real snapshot");
        }

        let chart = build_chart_points(&connection).expect("chart points");

        assert_eq!(chart.len(), MAX_CHART_POINTS);
        assert_eq!(
            chart
                .iter()
                .map(|point| point.label.as_str())
                .collect::<Vec<_>>(),
            vec!["08:30", "09:00", "09:15", "09:30", "10:00", "10:15"]
        );
    }

    #[test]
    fn cleanup_legacy_demo_data_removes_mock_accounts_and_snapshots() {
        let connection = setup_test_connection();
        connection
            .execute(
                "INSERT INTO accounts (provider, nickname, status, is_active, is_default, auth_state, last_check_time, estimated_reset_time, account_key, binding_kind, session_ref, profile_ref, account_email, last_verified_at, is_real_session, created_at, updated_at)
                 VALUES ('Codex', '模拟账号', 'healthy', 0, 0, 'valid', NULL, NULL, 'mock-account', 'manual', 'seed://mock', NULL, NULL, NULL, 0, ?1, ?1)",
                params![now_text()],
            )
            .expect("insert mock account");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (1, ?1, 80, 50, 'warning', NULL, NULL, 'mock_estimator', '本地估算', 1, '{}')",
                params![now_text()],
            )
            .expect("insert mock snapshot");
        cleanup_legacy_demo_data(&connection).expect("cleanup legacy demo data");

        let manual_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM accounts WHERE is_real_session = 0 OR binding_kind = 'manual'", [], |row| row.get(0))
            .expect("count manual accounts");
        let mock_snapshot_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_snapshots WHERE source_type != 'real_usage' OR is_estimated = 1", [], |row| row.get(0))
            .expect("count mock snapshots");

        assert_eq!(manual_count, 0);
        assert_eq!(mock_snapshot_count, 0);
    }

    #[test]
    fn chart_points_show_unknown_when_real_accounts_have_no_real_history() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots", [])
            .expect("clear snapshots");
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', updated_at = ?1",
                params![now_text()],
            )
            .expect("mark all accounts real");

        let chart = build_chart_points(&connection).expect("chart points");
        assert_eq!(chart.len(), 1);
        assert_eq!(chart[0].label, "现在");
        assert_eq!(chart[0].source_label, "未知");
        assert_eq!(chart[0].series.len(), 0);
    }

    #[test]
    fn latest_display_snapshot_hides_estimated_data_for_real_accounts() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 1", [])
            .expect("clear account snapshots");

        let real_account = query_account_by_id(&connection, 1).expect("real account");
        assert!(latest_display_snapshot(&connection, &real_account)
            .expect("real display snapshot")
            .is_none());

        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (?1, ?2, 28, 16, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"test\"}')",
                params![1, now_text()],
            )
            .expect("insert real usage snapshot");

        let visible_snapshot = latest_display_snapshot(&connection, &real_account)
            .expect("real usage display snapshot")
            .expect("visible real usage snapshot");
        assert_eq!(visible_snapshot.source_type, "real_usage");
        assert!(!visible_snapshot.is_estimated);
    }

    #[test]
    fn recommendations_for_real_account_without_snapshot_stay_conservative() {
        let connection = setup_test_connection();
        let settings = query_settings(&connection).expect("settings");
        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 1", [])
            .expect("clear account snapshots");

        let accounts = query_accounts(&connection).expect("accounts");
        let active_account = accounts
            .iter()
            .find(|account| account.id == 1)
            .expect("active account");
        let (recommendations, recommended_account_id, recommended_reason) = build_recommendations(
            &connection,
            &accounts,
            Some(active_account),
            None,
            &settings,
        )
        .expect("recommendations");

        assert!(recommendations
            .iter()
            .any(|item| item.contains("没有稳定真实额度快照")));
        assert_eq!(recommended_account_id, None);
        assert!(recommended_reason.is_none());
        assert!(!recommendations
            .iter()
            .any(|item| item.contains("历史本地记录")));
    }

    #[test]
    fn recommendations_use_only_real_verified_candidates() {
        let connection = setup_test_connection();
        let settings = query_settings(&connection).expect("settings");
        connection
            .execute(
                "UPDATE accounts SET status = 'exhausted', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark account b exhausted");

        let accounts = query_accounts(&connection).expect("accounts");
        let active_account = accounts
            .iter()
            .find(|account| account.id == 1)
            .expect("active account");
        let (recommendations, recommended_account_id, _) = build_recommendations(
            &connection,
            &accounts,
            Some(active_account),
            None,
            &settings,
        )
        .expect("recommendations");

        assert_eq!(recommended_account_id, None);
        assert!(build_recommendations(
            &connection,
            &accounts,
            Some(active_account),
            None,
            &settings
        )
        .expect("recommendations")
        .2
        .is_none());
        assert!(!recommendations
            .iter()
            .any(|item| item.contains("历史本地记录")));
    }

    #[test]
    fn mismatch_account_requires_rebind_before_switch() {
        let connection = setup_test_connection();
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', auth_state = 'mismatch', status = 'healthy', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark account mismatch");

        let target = query_account_by_id(&connection, 2).expect("target account");
        let reason = is_switchable(&target).expect_err("should block switching");
        assert!(reason.contains("重新绑定"));
    }

    #[test]
    fn sample_real_account_usage_reuses_latest_real_snapshot() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots", [])
            .expect("clear snapshots");
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', auth_state = 'valid', status = 'healthy', updated_at = ?1 WHERE id = 1",
                params![now_text()],
            )
            .expect("mark account real");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (1, ?1, 31, 18, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"seed\"}')",
                params![now_text()],
            )
            .expect("seed real snapshot");

        let latest = query_latest_real_usage_snapshot(&connection, 1)
            .expect("query latest real snapshot")
            .expect("snapshot");
        assert_eq!(latest.window_5h_percent, 31);
        assert_eq!(latest.window_7d_percent, 18);
        assert_eq!(latest.source_type, "real_usage");
    }

    #[test]
    fn read_real_usage_for_bound_account_errors_without_available_session() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 1", [])
            .expect("clear account snapshots");
        // Disable the Keychain fallback so this fixture has no available session.
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', auth_state = 'valid', status = 'healthy', session_ref = 'missing-profile.json', account_key = '', updated_at = ?1 WHERE id = 1",
                params![now_text()],
            )
            .expect("mark account real");

        let account = query_account_by_id(&connection, 1).expect("real account");
        let reading = read_real_usage_for_bound_account(&connection, &account);
        assert!(reading.is_err());
    }

    #[test]
    fn validate_post_switch_checks_real_accounts() {
        let connection = setup_test_connection();
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', session_ref = 'missing-profile.json', auth_state = 'valid', status = 'healthy', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark real account");

        let live_snapshot = SessionSnapshot {
            session_ref: "keychain://live-test".to_string(),
            credentials_json: "{\"loggedIn\":true}".to_string(),
        };
        let result = validate_post_switch(&connection, 2, Some(&live_snapshot));
        assert!(result.is_err());
    }

    #[test]
    fn resilient_sampler_continues_after_account_failure() {
        let accounts = vec![
            Account {
                id: 1,
                provider: "Codex".into(),
                nickname: "账号 A".into(),
                status: "healthy".into(),
                is_active: true,
                is_default: true,
                auth_state: "valid".into(),
                last_check_time: None,
                estimated_reset_time: None,
                account_key: "a".into(),
                binding_kind: "codex_cli".into(),
                session_ref: "keychain://a".into(),
                profile_ref: None,
                account_email: None,
                last_verified_at: None,
                is_real_session: true,
                plan_label: None,
                latest_snapshot: None,
            },
            Account {
                id: 2,
                provider: "Codex".into(),
                nickname: "账号 B".into(),
                status: "healthy".into(),
                is_active: false,
                is_default: false,
                auth_state: "valid".into(),
                last_check_time: None,
                estimated_reset_time: None,
                account_key: "b".into(),
                binding_kind: "codex_cli".into(),
                session_ref: "keychain://b".into(),
                profile_ref: None,
                account_email: None,
                last_verified_at: None,
                is_real_session: true,
                plan_label: None,
                latest_snapshot: None,
            },
            Account {
                id: 3,
                provider: "Codex".into(),
                nickname: "账号 C".into(),
                status: "healthy".into(),
                is_active: false,
                is_default: false,
                auth_state: "valid".into(),
                last_check_time: None,
                estimated_reset_time: None,
                account_key: "c".into(),
                binding_kind: "codex_cli".into(),
                session_ref: "keychain://c".into(),
                profile_ref: None,
                account_email: None,
                last_verified_at: None,
                is_real_session: true,
                plan_label: None,
                latest_snapshot: None,
            },
        ];
        let mut visited = Vec::new();

        let failures = run_resilient_sampling_cycle(&accounts, |account| {
            visited.push(account.nickname.clone());
            if account.id == 2 {
                Err("请求真实额度接口失败".to_string())
            } else {
                Ok(true)
            }
        });

        assert_eq!(visited, vec!["账号 A", "账号 B", "账号 C"]);
        assert_eq!(failures, vec!["账号 B：请求真实额度接口失败"]);
    }

    #[test]
    fn sampling_failure_summary_lists_failed_accounts() {
        let summary = summarize_sampling_failures(&[
            "账号 B：请求真实额度接口失败".to_string(),
            "账号 C：真实额度接口返回状态 503".to_string(),
        ]);

        assert_eq!(
            summary,
            "2 个账号采样失败：账号 B：请求真实额度接口失败；账号 C：真实额度接口返回状态 503"
        );
    }

    #[test]
    fn auth_invalid_sampling_error_marks_account_expired() {
        let connection = setup_test_connection();
        let account = query_account_by_id(&connection, 1).expect("account");
        let sampled_at = now_text();

        let result = apply_real_usage_read_error(
            &connection,
            &account,
            &sampled_at,
            usage::RealUsageReadError {
                kind: usage::RealUsageReadErrorKind::AuthInvalid,
                message: "真实额度接口返回状态 403 Forbidden".to_string(),
            },
        );

        assert!(result.is_err());
        let refreshed = query_account_by_id(&connection, 1).expect("refreshed");
        assert_eq!(refreshed.status, "auth_invalid");
        assert_eq!(refreshed.auth_state, "expired");
        assert_eq!(
            refreshed.last_check_time.as_deref(),
            Some(sampled_at.as_str())
        );
    }

    #[test]
    fn inactive_account_auth_invalid_sampling_keeps_previous_bind_state() {
        let connection = setup_test_connection();
        let account = query_account_by_id(&connection, 2).expect("inactive account");
        assert!(!account.is_active);
        assert_eq!(account.status, "healthy");
        assert_eq!(account.auth_state, "valid");

        let mut created_notifications = 0;
        apply_background_sampling_outcome(
            &connection,
            &account,
            Local::now(),
            RealAccountSamplingOutcome::Expired,
            &mut created_notifications,
            3,
        )
        .expect("apply non-active auth invalid sampling outcome");

        let refreshed = query_account_by_id(&connection, 2).expect("refreshed account");
        assert_eq!(refreshed.status, "healthy");
        assert_eq!(refreshed.auth_state, "valid");
        assert_eq!(created_notifications, 1);
    }

    #[test]
    fn sampling_records_remaining_decrease_before_reset() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 1", [])
            .expect("clear snapshots");
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', auth_state = 'valid', status = 'healthy', updated_at = ?1 WHERE id = 1",
                params![now_text()],
            )
            .expect("mark account real");

        let now = Local::now();
        let reset_5h = (now + Duration::hours(4))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let reset_7d = (now + Duration::days(4))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (1, ?1, 9, 21, 'healthy', ?2, ?3, 'real_usage', '精确', 0, '{\"kind\":\"stable\"}')",
                params![now_text(), reset_5h, reset_7d],
            )
            .expect("insert stable snapshot");
        let account = query_account_by_id(&connection, 1).expect("real account");
        let mut created_notifications = 0;

        apply_background_sampling_outcome(
            &connection,
            &account,
            now,
            RealAccountSamplingOutcome::Updated(RealUsageReading {
                window_5h_percent: 1,
                window_7d_percent: 20,
                confidence_label: "精确".to_string(),
                estimated_reset_5h_at: Some(
                    (now + Duration::hours(5))
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                ),
                estimated_reset_7d_at: Some(reset_7d),
                raw_meta_json: "{\"kind\":\"regression\"}".to_string(),
            }),
            &mut created_notifications,
            3,
        )
        .expect("apply lower remaining sampling outcome");

        let latest = query_latest_real_usage_snapshot(&connection, 1)
            .expect("query latest")
            .expect("latest snapshot");
        assert_eq!(latest.window_5h_percent, 1);
        assert_eq!(latest.window_7d_percent, 20);
        assert_eq!(created_notifications, 0);
    }

    #[test]
    fn sampling_accepts_cross_window_change_after_account_switch() {
        let connection = setup_test_connection();
        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 1", [])
            .expect("clear snapshots");
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', auth_state = 'valid', status = 'healthy', updated_at = ?1 WHERE id = 1",
                params![now_text()],
            )
            .expect("mark account real");

        let now = Local::now();
        let reset_5h = (now + Duration::hours(4))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let reset_7d = (now + Duration::days(4))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (1, ?1, 1, 72, 'healthy', ?2, ?3, 'real_usage', '精确', 0, '{\"kind\":\"old-context\"}')",
                params![now_text(), reset_5h, reset_7d],
            )
            .expect("insert old context snapshot");
        let account = query_account_by_id(&connection, 1).expect("real account");
        let mut created_notifications = 0;

        apply_background_sampling_outcome(
            &connection,
            &account,
            now,
            RealAccountSamplingOutcome::Updated(RealUsageReading {
                window_5h_percent: 90,
                window_7d_percent: 32,
                confidence_label: "精确".to_string(),
                estimated_reset_5h_at: Some(
                    (now + Duration::hours(5))
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                ),
                estimated_reset_7d_at: Some(reset_7d),
                raw_meta_json: "{\"kind\":\"current-context\"}".to_string(),
            }),
            &mut created_notifications,
            3,
        )
        .expect("apply cross-window sampling outcome");

        let latest = query_latest_real_usage_snapshot(&connection, 1)
            .expect("query latest")
            .expect("latest snapshot");
        assert_eq!(latest.window_5h_percent, 90);
        assert_eq!(latest.window_7d_percent, 32);
        assert_eq!(created_notifications, 0);
    }

    #[test]
    fn query_accounts_recovers_inactive_auth_invalid_when_bound_snapshot_is_trusted() {
        let connection = setup_test_connection();
        let snapshot_path = unique_temp_file("inactive-trusted-snapshot.json");
        fs::write(
            &snapshot_path,
            r#"{"tokens":{"account_id":"00000000-0000-4000-8000-000000000002"}}"#,
        )
        .expect("write trusted snapshot");
        let sample_at = now_text();
        connection
            .execute(
                "UPDATE accounts
                 SET status = 'auth_invalid',
                     auth_state = 'expired',
                     is_active = 0,
                     account_key = 'codex-00000000-0000-4000-8000-000000000002',
                     session_ref = ?1,
                     profile_ref = '00000000-0000-4000-8000-000000000002',
                     account_email = 'inactive@example.com'
                 WHERE id = 2",
                [snapshot_path.to_string_lossy().to_string()],
            )
            .expect("mark inactive account expired");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (2, ?1, 12, 34, 'healthy', NULL, NULL, 'real_usage', '精确', 0, '{\"kind\":\"trusted\"}')",
                [sample_at],
            )
            .expect("insert trusted snapshot");

        let recovered = query_accounts(&connection)
            .expect("query accounts")
            .into_iter()
            .find(|account| account.id == 2)
            .expect("account 2");

        assert_eq!(recovered.status, "healthy");
        assert_eq!(recovered.auth_state, "valid");
        let _ = fs::remove_file(snapshot_path);
    }

    #[test]
    fn expired_exhausted_reset_window_becomes_switchable_warning() {
        let connection = setup_test_connection();
        let past_reset = (Local::now() - Duration::minutes(5))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 2", [])
            .expect("clear account snapshots");
        connection
            .execute(
                "UPDATE accounts SET status = 'exhausted', auth_state = 'valid', is_real_session = 1, binding_kind = 'codex_cli', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark account exhausted");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (2, ?1, 100, 0, 'exhausted', ?2, NULL, 'real_usage', '精确', 0, '{\"kind\":\"reset_due\"}')",
                params![now_text(), past_reset],
            )
            .expect("insert exhausted snapshot");

        let recovered = query_account_by_id(&connection, 2).expect("query account");

        assert_eq!(recovered.status, "warning");
        assert!(is_switchable(&recovered).is_ok());
    }

    #[test]
    fn future_exhausted_reset_window_stays_blocked() {
        let connection = setup_test_connection();
        let future_reset = (Local::now() + Duration::minutes(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        connection
            .execute("DELETE FROM usage_snapshots WHERE account_id = 2", [])
            .expect("clear account snapshots");
        connection
            .execute(
                "UPDATE accounts SET status = 'exhausted', auth_state = 'valid', is_real_session = 1, binding_kind = 'codex_cli', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark account exhausted");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (2, ?1, 100, 0, 'exhausted', ?2, NULL, 'real_usage', '精确', 0, '{\"kind\":\"reset_pending\"}')",
                params![now_text(), future_reset],
            )
            .expect("insert exhausted snapshot");

        let blocked = query_account_by_id(&connection, 2).expect("query account");

        assert_eq!(blocked.status, "exhausted");
        assert!(is_switchable(&blocked).is_err());
    }

    #[test]
    fn sampling_run_guard_prevents_reentry_until_released() {
        let state = AppState {
            db: Mutex::new(Connection::open_in_memory().expect("memory db")),
            sampling_in_progress: std::sync::atomic::AtomicBool::new(false),
        };

        let guard = try_begin_sampling_run(&state).expect("first run should start");
        assert!(try_begin_sampling_run(&state).is_none());
        drop(guard);
        assert!(try_begin_sampling_run(&state).is_some());
    }

    #[test]
    fn automatic_sampling_includes_inactive_account_when_reset_is_due() {
        let connection = setup_test_connection();
        let now = Local::now();
        let sample_time = (now - Duration::hours(5))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let reset_due = (now - Duration::minutes(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let reset_future = (now + Duration::days(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', is_active = 0, auth_state = 'valid', status = 'warning', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark inactive real account");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (2, ?1, 0, 63, 'exhausted', ?2, ?3, 'real_usage', '精确', 0, '{\"kind\":\"due\"}')",
                params![sample_time, reset_due, reset_future],
            )
            .expect("insert due snapshot");

        let accounts = query_accounts(&connection).expect("accounts");
        let selected = automatic_sampling_accounts(&accounts, now);
        assert!(selected.iter().any(|account| account.id == 2));
    }

    #[test]
    fn automatic_sampling_skips_inactive_account_before_reset() {
        let connection = setup_test_connection();
        let now = Local::now();
        let sample_time = (now - Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let reset_future = (now + Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        connection
            .execute(
                "UPDATE accounts SET is_real_session = 1, binding_kind = 'codex_cli', is_active = 0, auth_state = 'valid', status = 'warning', updated_at = ?1 WHERE id = 2",
                params![now_text()],
            )
            .expect("mark inactive real account");
        connection
            .execute(
                "INSERT INTO usage_snapshots (account_id, sample_time, window_5h_percent, window_7d_percent, risk_level, estimated_reset_5h_at, estimated_reset_7d_at, source_type, confidence_level, is_estimated, raw_meta_json)
                 VALUES (2, ?1, 0, 63, 'exhausted', ?2, ?2, 'real_usage', '精确', 0, '{\"kind\":\"not-due\"}')",
                params![sample_time, reset_future],
            )
            .expect("insert not due snapshot");

        let accounts = query_accounts(&connection).expect("accounts");
        let selected = automatic_sampling_accounts(&accounts, now);
        assert!(!selected.iter().any(|account| account.id == 2));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            fs::create_dir_all(&app_dir).map_err(|error| error.to_string())?;
            let db_path = app_dir.join("codexswitcher-mac.db");
            let connection = Connection::open(db_path).map_err(|error| error.to_string())?;
            init_database(&connection)?;
            let _ = reconcile_runtime_active_identity(&connection);
            let _ = ensure_one_active(&connection);
            let _ = backfill_codex_imported_state_model_providers(&connection);
            let _ = sync_codex_thread_visibility_for_active_owner(&connection);
            app.manage(AppState {
                db: Mutex::new(connection),
                sampling_in_progress: AtomicBool::new(false),
            });
            setup_tray(app)?;
            setup_background_sampler(app);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            list_accounts,
            list_credential_profiles,
            get_key_profile_usage,
            update_key_profile_usage_config,
            create_key_profile,
            update_key_profile,
            activate_credential_profile,
            delete_credential_profile,
            get_account_detail,
            start_codex_login_flow,
            bind_current_codex_account,
            diagnose_bind_environment_command,
            verify_bound_account,
            delete_account,
            set_default_account,
            repair_account_auth,
            get_dashboard_overview,
            get_workspace_support_data,
            trigger_usage_sampling,
            switch_account,
            list_local_projects,
            list_session_records,
            list_sessions_for_profile,
            import_codex_local_sessions,
            list_codex_local_session_candidates,
            import_codex_local_session_candidates,
            list_notifications,
            get_release_diagnostic,
            get_startup_health,
            preview_cleanup_debug_data,
            cleanup_debug_data,
            update_settings,
            open_main_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Ready = event {
                match ensure_main_window(app) {
                    Ok(window) => {
                        if let Err(error) = show_window(&window) {
                            eprintln!("CodexSwitcherMac show window failed: {}", error);
                        }
                    }
                    Err(error) => eprintln!("CodexSwitcherMac ensure window failed: {}", error),
                }
            }
        });
}
