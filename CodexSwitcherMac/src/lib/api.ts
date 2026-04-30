import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountDetail,
  AppSettings,
  BindCurrentCodexAccountInput,
  BindEnvironmentDiagnostic,
  BootstrapState,
  CleanupPreview,
  CleanupResult,
  CodexLocalSessionCandidate,
  CodexLocalSessionImportResult,
  CreateKeyProfileInput,
  CredentialProfile,
  DashboardOverview,
  LocalProject,
  NotificationItem,
  ReleaseDiagnostic,
  SessionRecord,
  StartupHealth,
  ThirdPartyKeyUsageSummary,
  UpdateKeyProfileUsageConfigInput,
  UpdateKeyProfileInput,
  WorkspaceSupportData,
} from "../types";
import { browserPreviewApi } from "./browserPreview";

function hasTauriRuntime() {
  return typeof window !== "undefined" && Boolean((window as { __TAURI_INTERNALS__?: { invoke?: unknown } }).__TAURI_INTERNALS__?.invoke);
}

async function invokeOrPreview<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  preview: () => T | Promise<T>,
) {
  if (hasTauriRuntime()) {
    return invoke<T>(command, args);
  }
  return preview();
}

export const api = {
  bootstrap: () => invokeOrPreview<BootstrapState>("get_bootstrap_state", undefined, () => browserPreviewApi.bootstrap()),
  listAccounts: () => invokeOrPreview<Account[]>("list_accounts", undefined, () => browserPreviewApi.listAccounts()),
  listCredentialProfiles: () =>
    invokeOrPreview<CredentialProfile[]>(
      "list_credential_profiles",
      undefined,
      () => browserPreviewApi.listCredentialProfiles(),
    ),
  getKeyProfileUsage: (profileId: number) =>
    invokeOrPreview<ThirdPartyKeyUsageSummary | null>(
      "get_key_profile_usage",
      { profileId },
      () => browserPreviewApi.getKeyProfileUsage(profileId),
    ),
  updateKeyProfileUsageConfig: (input: UpdateKeyProfileUsageConfigInput) =>
    invokeOrPreview<CredentialProfile>(
      "update_key_profile_usage_config",
      { input },
      () => browserPreviewApi.updateKeyProfileUsageConfig(input),
    ),
  createKeyProfile: (input: CreateKeyProfileInput) =>
    invokeOrPreview<CredentialProfile>(
      "create_key_profile",
      { input },
      () => browserPreviewApi.createKeyProfile(input),
    ),
  updateKeyProfile: (input: UpdateKeyProfileInput) =>
    invokeOrPreview<CredentialProfile>(
      "update_key_profile",
      { input },
      () => browserPreviewApi.updateKeyProfile(input),
    ),
  activateCredentialProfile: (profileId: number) =>
    invokeOrPreview<CredentialProfile>(
      "activate_credential_profile",
      { profileId },
      () => browserPreviewApi.activateCredentialProfile(profileId),
    ),
  getAccountDetail: (id: number) =>
    invokeOrPreview<AccountDetail>("get_account_detail", { id }, () => browserPreviewApi.getAccountDetail(id)),
  startCodexLoginFlow: () =>
    invokeOrPreview<string>("start_codex_login_flow", undefined, () => browserPreviewApi.startCodexLoginFlow()),
  bindCurrentCodexAccount: (input: BindCurrentCodexAccountInput) =>
    invokeOrPreview<Account>("bind_current_codex_account", { input }, () => browserPreviewApi.bindCurrentCodexAccount(input)),
  diagnoseBindEnvironment: () =>
    invokeOrPreview<BindEnvironmentDiagnostic>(
      "diagnose_bind_environment_command",
      undefined,
      () => browserPreviewApi.diagnoseBindEnvironment(),
    ),
  verifyBoundAccount: (id: number) =>
    invokeOrPreview<Account>("verify_bound_account", { id }, () => browserPreviewApi.verifyBoundAccount(id)),
  deleteAccount: (id: number) => invokeOrPreview<void>("delete_account", { id }, () => browserPreviewApi.deleteAccount(id)),
  setDefaultAccount: (id: number) =>
    invokeOrPreview<Account>("set_default_account", { id }, () => browserPreviewApi.setDefaultAccount(id)),
  repairAccountAuth: (id: number) =>
    invokeOrPreview<Account>("repair_account_auth", { id }, () => browserPreviewApi.repairAccountAuth(id)),
  getOverview: () =>
    invokeOrPreview<DashboardOverview>("get_dashboard_overview", undefined, () => browserPreviewApi.getOverview()),
  getWorkspaceSupportData: () =>
    invokeOrPreview<WorkspaceSupportData>(
      "get_workspace_support_data",
      undefined,
      () => browserPreviewApi.getWorkspaceSupportData(),
    ),
  triggerSampling: () =>
    invokeOrPreview<DashboardOverview>("trigger_usage_sampling", undefined, () => browserPreviewApi.triggerSampling()),
  switchAccount: (targetAccountId: number) =>
    invokeOrPreview<DashboardOverview>(
      "switch_account",
      { targetAccountId },
      () => browserPreviewApi.switchAccount(targetAccountId),
    ),
  listLocalProjects: () =>
    invokeOrPreview<LocalProject[]>("list_local_projects", undefined, () => browserPreviewApi.listLocalProjects()),
  listSessionRecords: () =>
    invokeOrPreview<SessionRecord[]>("list_session_records", undefined, () => browserPreviewApi.listSessionRecords()),
  listSessionsForProfile: (profileKind: string, profileRef: string) =>
    invokeOrPreview<SessionRecord[]>(
      "list_sessions_for_profile",
      { profileKind, profileRef },
      () => browserPreviewApi.listSessionsForProfile(profileKind, profileRef),
    ),
  importCodexLocalSessions: () =>
    invokeOrPreview<CodexLocalSessionImportResult>(
      "import_codex_local_sessions",
      undefined,
      () => browserPreviewApi.importCodexLocalSessions(),
    ),
  listCodexLocalSessionCandidates: () =>
    invokeOrPreview<CodexLocalSessionCandidate[]>(
      "list_codex_local_session_candidates",
      undefined,
      () => browserPreviewApi.listCodexLocalSessionCandidates(),
    ),
  importCodexLocalSessionCandidates: (candidateIds: string[]) =>
    invokeOrPreview<CodexLocalSessionImportResult>(
      "import_codex_local_session_candidates",
      { candidateIds },
      () => browserPreviewApi.importCodexLocalSessionCandidates(candidateIds),
    ),
  listNotifications: () =>
    invokeOrPreview<NotificationItem[]>("list_notifications", undefined, () => browserPreviewApi.listNotifications()),
  getReleaseDiagnostic: () =>
    invokeOrPreview<ReleaseDiagnostic>("get_release_diagnostic", undefined, () => browserPreviewApi.getReleaseDiagnostic()),
  getStartupHealth: () =>
    invokeOrPreview<StartupHealth>("get_startup_health", undefined, () => browserPreviewApi.getStartupHealth()),
  previewCleanupDebugData: () =>
    invokeOrPreview<CleanupPreview>("preview_cleanup_debug_data", undefined, () => browserPreviewApi.previewCleanupDebugData()),
  cleanupDebugData: () =>
    invokeOrPreview<CleanupResult>("cleanup_debug_data", undefined, () => browserPreviewApi.cleanupDebugData()),
  updateSettings: (settings: AppSettings) =>
    invokeOrPreview<AppSettings>("update_settings", { settings }, () => browserPreviewApi.updateSettings(settings)),
  openMainWindow: () => invokeOrPreview<void>("open_main_window", undefined, () => browserPreviewApi.openMainWindow()),
};
