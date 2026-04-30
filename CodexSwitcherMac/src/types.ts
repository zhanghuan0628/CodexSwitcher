export type AccountStatus = "healthy" | "warning" | "exhausted" | "auth_invalid" | "error";
export type AccountAuthState = "valid" | "mismatch" | "expired" | "unknown";
export type AccountPlanLabel = "plus" | "pro" | "team";

export type AccountBindingKind = "codex_cli" | "manual";
export type UsageDisplayStatus = "ready" | "unknown";
export type UsageSourceType = "real_usage" | "unknown";
export type UsageConfidenceLabel = "精确" | "未知" | "待校验";
export type SamplingResultKind = "real_updated" | "real_unknown" | "idle";
export type NotificationSourceType =
  | "real_login"
  | "real_binding"
  | "real_verification"
  | "real_repair"
  | "real_switch"
  | "settings_event"
  | "system";

export interface Account {
  id: number;
  provider: string;
  nickname: string;
  status: AccountStatus;
  is_active: boolean;
  is_default: boolean;
  auth_state: AccountAuthState;
  last_check_time: string | null;
  estimated_reset_time: string | null;
  account_key: string;
  binding_kind: AccountBindingKind;
  session_ref: string;
  profile_ref: string | null;
  account_email: string | null;
  last_verified_at: string | null;
  is_real_session: boolean;
  plan_label: AccountPlanLabel | null;
  latest_snapshot: UsageSnapshot | null;
}

export interface CredentialProfile {
  id: number;
  profile_kind: "official_account" | "third_party_key" | string;
  provider: string;
  nickname: string;
  status: string;
  is_active: boolean;
  base_url: string | null;
  model: string | null;
  masked_secret: string | null;
  secret_ref: string | null;
  linked_account_id: number | null;
  usage_provider_type: "none" | "sub2api" | "new_api" | string | null;
  usage_query_user: string | null;
  usage_query_app_version: string | null;
  usage_masked_secret: string | null;
  usage_summary?: ThirdPartyKeyUsageSummary | null;
}

export interface ThirdPartyKeyUsageDetailItem {
  label: string;
  value: string;
}

export interface ThirdPartyKeyUsageBucket {
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  total_tokens: number;
  cost: number;
  actual_cost: number;
  account_cost: number | null;
}

export interface ThirdPartyKeyUsageModelStat {
  model: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  total_tokens: number;
  cost: number;
  actual_cost: number;
  account_cost: number | null;
}

export interface ThirdPartyKeyUsageSummary {
  status: "ready" | "error" | string;
  message: string | null;
  fetched_at: string;
  usage_endpoint: string | null;
  usage_provider_type: string | null;
  balance: number | null;
  remaining: number | null;
  unit: string | null;
  is_valid: boolean | null;
  mode: string | null;
  plan_name: string | null;
  average_duration_ms: number | null;
  rpm: number | null;
  tpm: number | null;
  today: ThirdPartyKeyUsageBucket | null;
  total: ThirdPartyKeyUsageBucket | null;
  model_stats: ThirdPartyKeyUsageModelStat[];
  detail_items: ThirdPartyKeyUsageDetailItem[];
}

export interface CreateKeyProfileInput {
  provider: string;
  nickname: string;
  base_url: string;
  model: string;
  api_key: string;
  usage_provider_type: "none" | "sub2api" | "new_api";
  usage_query_user: string;
  usage_query_app_version: string;
  usage_access_token: string;
}

export interface UpdateKeyProfileInput extends CreateKeyProfileInput {
  id: number;
}

export interface UpdateKeyProfileUsageConfigInput {
  profile_id: number;
  usage_provider_type: "none" | "sub2api" | "new_api";
  usage_query_user: string;
  usage_query_app_version: string;
  usage_access_token: string;
}

export interface AppSettings {
  warn_threshold_low: number;
  warn_threshold_mid: number;
  warn_threshold_high: number;
  check_interval: number;
  enable_handoff: boolean;
  prefer_official_upgrade: boolean;
  enable_auto_refresh: boolean;
  enable_auto_sampling: boolean;
  foreground_auto_sampling_only: boolean;
  launch_at_login: boolean;
  menu_bar_only: boolean;
}

export interface UsageSnapshot {
  account_id: number;
  sample_time: string;
  window_5h_percent: number;
  window_7d_percent: number;
  risk_level: AccountStatus;
  estimated_reset_5h_at: string | null;
  estimated_reset_7d_at: string | null;
  source_type: UsageSourceType;
  confidence_level: UsageConfidenceLabel | string;
  is_estimated: boolean;
}

export interface SwitchLog {
  id: number;
  from_account_id: number | null;
  to_account_id: number;
  result: string;
  reason: string;
  created_at: string;
}

export interface LocalProject {
  id: number;
  name: string;
  workspace_path: string;
  git_remote: string | null;
  last_active_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface SessionRecord {
  id: number;
  project_id: number;
  project_name: string;
  project_path: string;
  owner_account_id: number | null;
  owner_profile_kind: string;
  owner_profile_ref: string;
  record_type: string;
  title: string;
  summary: string;
  raw_content: string;
  message_count: number;
  source_record_id: number | null;
  created_at: string;
  updated_at: string;
}

export interface NotificationItem {
  id: number;
  account_id: number | null;
  level: "info" | "warning" | "error" | "success";
  title: string;
  message: string;
  source_type: NotificationSourceType;
  action_type: string;
  related_handoff_id: number | null;
  created_at: string;
}

export interface ChartSeriesValue {
  account_id: number;
  account_name: string;
  value: number;
}

export interface ChartPoint {
  label: string;
  series: ChartSeriesValue[];
  event_label: string | null;
  source_label: string;
}

export interface TimelineSegment {
  state: AccountStatus | "unknown";
  hours: number;
  label: string;
  tooltip: string;
}

export interface TimelineLane {
  account_id: number;
  account_name: string;
  confidence: string;
  next_action: string;
  segments: TimelineSegment[];
}

export interface AccountDetail {
  account: Account;
  recent_snapshots: UsageSnapshot[];
  recent_switches: SwitchLog[];
  recent_notifications: NotificationItem[];
  recent_sessions: SessionRecord[];
  keychain_readable: boolean;
  bound_snapshot_summary: string | null;
  last_failure_reason: string | null;
  health_timeline: TimelineSegment[];
  diagnostic_text: string;
}

export interface SamplingSummary {
  kind: SamplingResultKind;
  message: string;
  source_type: UsageSourceType;
}

export interface UsageDisplayState {
  status: UsageDisplayStatus;
  source_type: UsageSourceType;
  confidence_label: UsageConfidenceLabel | string;
  summary: string;
  helper_text: string;
  chart_helper_text: string;
}

export interface CurrentCodexLogin {
  logged_in: boolean;
  email: string | null;
  account_id: string | null;
  is_bound: boolean;
}

export interface DashboardOverview {
  active_account: Account | null;
  accounts: Account[];
  current_login: CurrentCodexLogin | null;
  latest_snapshot: UsageSnapshot | null;
  usage_display: UsageDisplayState;
  latest_sampling: SamplingSummary;
  chart_points: ChartPoint[];
  timeline: TimelineLane[];
  recommendations: string[];
  recommended_account_id: number | null;
  recommended_reason: string | null;
  switch_logs: SwitchLog[];
  settings: AppSettings;
}

export interface BootstrapState {
  overview: DashboardOverview;
  accounts: Account[];
  settings: AppSettings;
}

export interface WorkspaceSupportData {
  projects: LocalProject[];
  sessions: SessionRecord[];
  notifications: NotificationItem[];
}

export interface CodexLocalSessionImportResult {
  scanned_files: number;
  imported_sessions: number;
  updated_sessions: number;
  skipped_files: number;
  codex_synced_threads: number;
  codex_skipped_threads: number;
  project_count: number;
  session_count: number;
  message: string;
}

export interface CodexLocalSessionCandidate {
  candidate_id: string;
  identity_key: string;
  identity_label: string;
  identity_kind_label: string;
  project_name: string;
  project_path: string;
  title: string;
  message_count: number;
  source_path: string;
  created_at: string;
  updated_at: string;
  imported_session_id: number | null;
  imported_owner_profile_kind: string | null;
  imported_owner_profile_ref: string | null;
}

export interface BindCurrentCodexAccountInput {
  nickname: string;
}

export interface BindEnvironmentDiagnostic {
  codex_config_dir: string;
  auth_path: string | null;
  auth_exists: boolean;
  current_email: string | null;
  current_account_id: string | null;
  current_is_bound: boolean;
  cli_candidates: string[];
  cli_available: string | null;
  cli_status_ok: boolean;
  cli_stdout: string;
  cli_stderr: string;
}

export interface AccountDiagnostic {
  account_id: number;
  nickname: string;
  email: string | null;
  profile_ref: string | null;
  status: AccountStatus;
  auth_state: AccountAuthState;
  keychain_readable: boolean;
  latest_sample_at: string | null;
  latest_switch_at: string | null;
  advice: string;
}

export interface ReleaseDiagnostic {
  generated_at: string;
  codex_cli_available: boolean;
  codex_cli_path: string | null;
  current_login: CurrentCodexLogin | null;
  database_ok: boolean;
  account_count: number;
  latest_sampling: SamplingSummary;
  latest_switch: SwitchLog | null;
  accounts: AccountDiagnostic[];
}

export interface StartupHealthCheck {
  label: string;
  ok: boolean;
  detail: string;
}

export interface StartupHealth {
  generated_at: string;
  healthy: boolean;
  checks: StartupHealthCheck[];
}

export type StartupSummaryTone = "healthy" | "warning" | "error";
export type StartupSummaryAction = "accounts" | "stability";

export interface StartupSummaryItem {
  label: string;
  detail: string;
  tone: StartupSummaryTone;
  action: StartupSummaryAction;
}

export interface StartupSummary {
  title: string;
  summary: string;
  tone: StartupSummaryTone;
  generated_at: string | null;
  items: StartupSummaryItem[];
}

export interface CleanupPreview {
  old_handoff_count: number;
  old_notification_count: number;
  orphan_handoff_count: number;
}

export interface CleanupResult extends CleanupPreview {
  deleted_total: number;
}
