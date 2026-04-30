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
  CurrentCodexLogin,
  DashboardOverview,
  LocalProject,
  NotificationItem,
  ReleaseDiagnostic,
  SessionRecord,
  StartupHealth,
  SwitchLog,
  ThirdPartyKeyUsageSummary,
  UpdateKeyProfileInput,
  UpdateKeyProfileUsageConfigInput,
  UsageSnapshot,
  WorkspaceSupportData,
} from "../types";

type PreviewState = {
  accounts: Account[];
  cleanupPreview: CleanupPreview;
  credentialProfiles: CredentialProfile[];
  currentLogin: CurrentCodexLogin;
  latestSampling: DashboardOverview["latest_sampling"];
  localProjects: LocalProject[];
  nextAccountId: number;
  nextCredentialProfileId: number;
  nextNotificationId: number;
  nextSwitchId: number;
  notifications: NotificationItem[];
  sessionRecords: SessionRecord[];
  settings: AppSettings;
  startupHealth: StartupHealth;
  switchLogs: SwitchLog[];
};

const previewSettings: AppSettings = {
  warn_threshold_low: 70,
  warn_threshold_mid: 85,
  warn_threshold_high: 95,
  check_interval: 15,
  enable_handoff: true,
  prefer_official_upgrade: true,
  enable_auto_refresh: true,
  enable_auto_sampling: true,
  foreground_auto_sampling_only: false,
  launch_at_login: false,
  menu_bar_only: false,
};

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function nextId(items: Array<{ id: number }>) {
  return Math.max(0, ...items.map((item) => item.id)) + 1;
}

function sampleTime(label: string) {
  return `2026-04-21 ${label}`;
}

function makeSnapshot(
  accountId: number,
  sampleLabel: string,
  window5h: number,
  window7d: number,
  riskLevel: Account["status"],
  reset5h: string | null,
  reset7d: string | null,
): UsageSnapshot {
  return {
    account_id: accountId,
    sample_time: sampleTime(sampleLabel),
    window_5h_percent: window5h,
    window_7d_percent: window7d,
    risk_level: riskLevel,
    estimated_reset_5h_at: reset5h,
    estimated_reset_7d_at: reset7d,
    source_type: "real_usage",
    confidence_level: "精确",
    is_estimated: false,
  };
}

function seedAccounts(): Account[] {
  return [
    {
      id: 1,
      provider: "Codex",
      nickname: "7135",
      status: "warning",
      is_active: true,
      is_default: true,
      auth_state: "valid",
      last_check_time: sampleTime("10:36:10"),
      estimated_reset_time: "2026-04-21 13:40:00",
      account_key: "codex-7135",
      binding_kind: "codex_cli",
      session_ref: "keychain://codex-7135",
      profile_ref: "00000000-0000-4000-8000-000000000004",
      account_email: "demo7135@example.com",
      last_verified_at: sampleTime("10:18:05"),
      is_real_session: true,
      plan_label: "plus",
      latest_snapshot: makeSnapshot(1, "10:35:48", 82, 46, "warning", "2026-04-21 13:40:00", "2026-04-27 08:00:00"),
    },
    {
      id: 2,
      provider: "Codex",
      nickname: "gmail",
      status: "healthy",
      is_active: false,
      is_default: false,
      auth_state: "valid",
      last_check_time: sampleTime("10:30:12"),
      estimated_reset_time: "2026-04-21 12:10:00",
      account_key: "codex-gmail",
      binding_kind: "codex_cli",
      session_ref: "keychain://codex-gmail",
      profile_ref: "00000000-0000-4000-8000-000000000003",
      account_email: "demo-gmail@example.com",
      last_verified_at: sampleTime("09:52:41"),
      is_real_session: true,
      plan_label: "team",
      latest_snapshot: makeSnapshot(2, "10:30:00", 18, 12, "healthy", "2026-04-21 12:10:00", "2026-04-26 18:20:00"),
    },
    {
      id: 3,
      provider: "Codex",
      nickname: "2027",
      status: "auth_invalid",
      is_active: false,
      is_default: false,
      auth_state: "expired",
      last_check_time: sampleTime("09:56:00"),
      estimated_reset_time: null,
      account_key: "codex-2027",
      binding_kind: "codex_cli",
      session_ref: "keychain://codex-2027",
      profile_ref: "00000000-0000-4000-8000-000000000005",
      account_email: "ops2027@example.com",
      last_verified_at: sampleTime("09:30:15"),
      is_real_session: true,
      plan_label: "pro",
      latest_snapshot: makeSnapshot(3, "09:50:00", 94, 68, "auth_invalid", null, null),
    },
  ];
}

function profileFromAccount(account: Account): CredentialProfile {
  return {
    id: account.id,
    profile_kind: "official_account",
    provider: account.provider,
    nickname: account.nickname,
    status: account.status,
    is_active: account.is_active,
    base_url: null,
    model: null,
    masked_secret: null,
    secret_ref: null,
    linked_account_id: account.id,
    usage_provider_type: null,
    usage_query_user: null,
    usage_query_app_version: null,
    usage_masked_secret: null,
    usage_summary: null,
  };
}

function previewKeyUsageSummary(): ThirdPartyKeyUsageSummary {
  return {
    status: "ready",
    message: null,
    fetched_at: sampleTime("10:40:00"),
    usage_endpoint: "https://sub2api.yuchat.top/v1/usage",
    usage_provider_type: "sub2api",
    balance: 136.24714015,
    remaining: 136.24714015,
    unit: "USD",
    is_valid: true,
    mode: "unrestricted",
    plan_name: "钱包余额",
    average_duration_ms: 18968.198441895693,
    rpm: 0,
    tpm: 0,
    today: {
      requests: 963,
      input_tokens: 5111031,
      output_tokens: 315340,
      cache_creation_tokens: 0,
      cache_read_tokens: 117390208,
      total_tokens: 122816579,
      cost: 93.3136683,
      actual_cost: 186.6273366,
      account_cost: null,
    },
    total: {
      requests: 4621,
      input_tokens: 26638590,
      output_tokens: 3357106,
      cache_creation_tokens: 0,
      cache_read_tokens: 299514496,
      total_tokens: 329510192,
      cost: 238.247032,
      actual_cost: 437.73630735,
      account_cost: null,
    },
    model_stats: [
      {
        model: "gpt-5.4",
        requests: 3655,
        input_tokens: 21553299,
        output_tokens: 3043140,
        cache_creation_tokens: 0,
        cache_read_tokens: 182251520,
        total_tokens: 206847959,
        cost: 145.0932275,
        actual_cost: 251.46659,
        account_cost: 145.0932275,
      },
    ],
    detail_items: [
      { label: "今日请求", value: "963 次" },
      { label: "今日费用", value: "93.31" },
      { label: "累计请求", value: "4621 次" },
      { label: "累计费用", value: "238.25" },
    ],
  };
}

function seedState(): PreviewState {
  const accounts = seedAccounts();
  return {
    accounts,
    cleanupPreview: {
      old_handoff_count: 0,
      old_notification_count: 4,
      orphan_handoff_count: 0,
    },
    credentialProfiles: [
      ...accounts.map(profileFromAccount),
      {
        id: 100,
        profile_kind: "third_party_key",
        provider: "custom",
        nickname: "YuChat 备用 Key",
        status: "unknown",
        is_active: false,
        base_url: "https://sub2api.yuchat.top",
        model: "gpt-5-codex",
        masked_secret: "exam...7135",
        secret_ref: "keychain://preview-yuchat-key",
        linked_account_id: null,
        usage_provider_type: "sub2api",
        usage_query_user: null,
        usage_query_app_version: null,
        usage_masked_secret: null,
        usage_summary: previewKeyUsageSummary(),
      },
    ],
    currentLogin: {
      logged_in: true,
      email: "demo7135@example.com",
      account_id: "00000000-0000-4000-8000-000000000004",
      is_bound: true,
    },
    localProjects: [
      {
        id: 1,
        name: "CodexSwitcher",
        workspace_path: "/Users/admin/IdeaProjects/CodexSwitcher",
        git_remote: null,
        last_active_at: sampleTime("09:24:10"),
        created_at: sampleTime("09:24:10"),
        updated_at: sampleTime("09:24:10"),
      },
    ],
    sessionRecords: [
      {
        id: 2,
        project_id: 1,
        project_name: "CodexSwitcher",
        project_path: "/Users/admin/IdeaProjects/CodexSwitcher",
        owner_account_id: 1,
        owner_profile_kind: "official_account",
        owner_profile_ref: "account:1",
        record_type: "local_session",
        title: "项目会话预览",
        summary: "预览模式示例会话，真实数据由本地 session 导入后生成。",
        raw_content: "",
        message_count: 0,
        source_record_id: null,
        created_at: sampleTime("08:42:00"),
        updated_at: sampleTime("08:42:00"),
      },
    ],
    latestSampling: {
      kind: "real_updated",
      message: "最近一次采样已更新到真实数据。",
      source_type: "real_usage",
    },
    nextAccountId: 4,
    nextCredentialProfileId: 101,
    nextNotificationId: 5,
    nextSwitchId: 2,
    notifications: [
      {
        id: 1,
        account_id: 1,
        level: "warning",
        title: "7135 已进入预警区间",
        message: "5h 已用 82%，建议提前准备切换到 gmail。",
        source_type: "system",
        action_type: "review_recommendation",
        related_handoff_id: null,
        created_at: sampleTime("10:36:10"),
      },
      {
        id: 2,
        account_id: 2,
        level: "success",
        title: "gmail 最近验证通过",
        message: "目标账号登录态正常，可直接切换并自动采样。",
        source_type: "real_verification",
        action_type: "verify_bound_account",
        related_handoff_id: null,
        created_at: sampleTime("09:58:24"),
      },
      {
        id: 3,
        account_id: 1,
        level: "success",
        title: "项目会话库已就绪",
        message: "本地项目和会话记录将作为后续同步中心的数据源。",
        source_type: "system",
        action_type: "project_sessions_ready",
        related_handoff_id: null,
        created_at: sampleTime("09:24:12"),
      },
      {
        id: 4,
        account_id: 3,
        level: "error",
        title: "2027 登录态失效",
        message: "当前账号需要重新登录并按当前官方登录态重绑。",
        source_type: "real_repair",
        action_type: "repair_account_auth",
        related_handoff_id: null,
        created_at: sampleTime("09:08:31"),
      },
    ],
    settings: clone(previewSettings),
    startupHealth: {
      generated_at: sampleTime("10:40:00"),
      healthy: true,
      checks: [
        { label: "本地数据库", ok: true, detail: "数据库连接正常，可读取账号与采样记录。" },
        { label: "Codex CLI", ok: true, detail: "CLI 可用，已检测到官方登录信息。" },
        { label: "多账号主链路", ok: true, detail: "至少 2 个真实账号可用于切换与采样。" },
      ],
    },
    switchLogs: [
      {
        id: 1,
        from_account_id: 2,
        to_account_id: 1,
        result: "success",
        reason: "目标账号 7135 风险更低，切换后继续执行当前任务。",
        created_at: sampleTime("09:24:08"),
      },
    ],
  };
}

let state = seedState();

function activeAccount() {
  return state.accounts.find((account) => account.is_active) ?? null;
}

function recommendedAccount() {
  return state.accounts.find(
    (account) => !account.is_active && account.is_real_session && account.auth_state === "valid" && account.status !== "exhausted",
  ) ?? null;
}

function buildOverview(): DashboardOverview {
  const active = activeAccount();
  const recommended = recommendedAccount();

  return {
    active_account: active,
    accounts: clone(state.accounts),
    current_login: clone(state.currentLogin),
    latest_snapshot: active?.latest_snapshot ?? null,
    usage_display: {
      status: "ready",
      source_type: "real_usage",
      confidence_label: active?.latest_snapshot?.confidence_level ?? "精确",
      summary: active?.latest_snapshot ? `${active.nickname} 当前 5h 已用 ${active.latest_snapshot.window_5h_percent}%` : "暂无可展示用量",
      helper_text: active?.latest_snapshot ? "展示最近一次真实采样结果。" : "等待首次真实采样。",
      chart_helper_text: "按账号查看最近 4 个时间点的真实采样变化。",
    },
    latest_sampling: clone(state.latestSampling),
    chart_points: [
      {
        label: "08:30",
        series: [
          { account_id: 1, account_name: "7135", value: 54 },
          { account_id: 2, account_name: "gmail", value: 12 },
        ],
        event_label: null,
        source_label: "真实采样",
      },
      {
        label: "09:00",
        series: [
          { account_id: 1, account_name: "7135", value: 63 },
          { account_id: 2, account_name: "gmail", value: 16 },
          { account_id: 3, account_name: "2027", value: 74 },
        ],
        event_label: "重新校验",
        source_label: "真实采样",
      },
      {
        label: "09:30",
        series: [
          { account_id: 1, account_name: "7135", value: 71 },
          { account_id: 2, account_name: "gmail", value: 18 },
          { account_id: 3, account_name: "2027", value: 81 },
        ],
        event_label: null,
        source_label: "真实采样",
      },
      {
        label: "10:00",
        series: state.accounts
          .filter((account) => account.latest_snapshot)
          .map((account) => ({
            account_id: account.id,
            account_name: account.nickname,
            value: account.latest_snapshot?.window_5h_percent ?? 0,
          })),
        event_label: active?.status === "warning" ? "预警" : null,
        source_label: "真实采样",
      },
    ],
    timeline: state.accounts.map((account) => ({
      account_id: account.id,
      account_name: account.nickname,
      confidence: account.latest_snapshot?.confidence_level ?? "待校验",
      next_action:
        account.auth_state !== "valid"
          ? "下一步：重新登录并重绑"
          : account.status === "warning"
            ? "下一步：继续观察并准备切换"
            : "下一步：保持待命",
      segments: [
        {
          state: account.status,
          hours: 2,
          label: account.status === "healthy" ? "稳定" : account.status === "warning" ? "预警" : "异常",
          tooltip: `${account.nickname} 当前状态：${account.status}`,
        },
        {
          state: account.auth_state === "valid" ? "healthy" : "auth_invalid",
          hours: 4,
          label: account.auth_state === "valid" ? "可用" : "需重绑",
          tooltip: `${account.nickname} 登录状态：${account.auth_state}`,
        },
        {
          state: account.status === "warning" ? "warning" : "healthy",
          hours: 6,
          label: account.status === "warning" ? "观察" : "待命",
          tooltip: `${account.nickname} 下一阶段建议：${account.status === "warning" ? "继续观察并准备切换" : "保持待命"}`,
        },
      ],
    })),
    recommendations: recommended
      ? [
        `${recommended.nickname} 当前 5h 仅 ${recommended.latest_snapshot?.window_5h_percent ?? 0}% ，是更稳妥的下一张账号。`,
        `${recommended.nickname} 登录态有效，切换后可立即进行真实采样。`,
      ]
      : ["暂无更合适的切换目标。"],
    recommended_account_id: recommended?.id ?? null,
    recommended_reason: recommended ? `推荐切换到 ${recommended.nickname}，因为它当前容量更健康且登录态有效。` : null,
    switch_logs: clone(state.switchLogs),
    settings: clone(state.settings),
  };
}

function buildReleaseDiagnostic(): ReleaseDiagnostic {
  return {
    generated_at: sampleTime("10:40:00"),
    codex_cli_available: true,
    codex_cli_path: "/usr/local/bin/codex",
    current_login: clone(state.currentLogin),
    database_ok: true,
    account_count: state.accounts.length,
    latest_sampling: clone(state.latestSampling),
    latest_switch: clone(state.switchLogs[0] ?? null),
    accounts: state.accounts.map((account) => ({
      account_id: account.id,
      nickname: account.nickname,
      email: account.account_email,
      profile_ref: account.profile_ref,
      status: account.status,
      auth_state: account.auth_state,
      keychain_readable: account.auth_state === "valid",
      latest_sample_at: account.latest_snapshot?.sample_time ?? null,
      latest_switch_at: state.switchLogs.find((item) => item.to_account_id === account.id)?.created_at ?? null,
      advice:
        account.auth_state !== "valid"
          ? "建议重新登录并重绑后，再参与切换。"
          : account.status === "warning"
            ? "建议保留为当前账号，同时预备下一张健康账号。"
            : "当前状态稳定，可继续作为可切换账号。",
    })),
  };
}

function buildAccountDetail(id: number): AccountDetail {
  const account = state.accounts.find((item) => item.id === id);
  if (!account) {
    throw new Error(`未找到账号 #${id}`);
  }

  return {
    account: clone(account),
    recent_snapshots: [
      clone(account.latest_snapshot!),
      {
        ...clone(account.latest_snapshot!),
        sample_time: sampleTime("08:00:00"),
        window_5h_percent: Math.max(0, (account.latest_snapshot?.window_5h_percent ?? 0) - 12),
        window_7d_percent: Math.max(0, (account.latest_snapshot?.window_7d_percent ?? 0) - 4),
      },
    ],
    recent_switches: clone(state.switchLogs.filter((item) => item.to_account_id === id || item.from_account_id === id).slice(0, 5)),
    recent_notifications: clone(state.notifications.filter((item) => item.account_id === id).slice(0, 5)),
    recent_sessions: clone(state.sessionRecords.filter((item) => item.owner_account_id === id).slice(0, 5)),
    keychain_readable: account.auth_state === "valid",
    bound_snapshot_summary: account.latest_snapshot ? `${account.nickname} 最近一次采样来自真实用量。` : null,
    last_failure_reason: account.auth_state === "valid" ? null : "当前账号登录态失效，需要重新登录。",
    health_timeline: buildOverview().timeline.find((item) => item.account_id === id)?.segments ?? [],
    diagnostic_text: [
      `账号：${account.nickname}`,
      `邮箱：${account.account_email ?? "未读取"}`,
      `官方账号 ID：${account.profile_ref ?? "--"}`,
      `状态：${account.status}`,
      `登录态：${account.auth_state}`,
      `最近采样：${account.latest_snapshot?.sample_time ?? "暂无"}`,
    ].join("\n"),
  };
}

function syncLoginFromActive() {
  const active = activeAccount();
  if (!active) {
    return;
  }
  state.currentLogin = {
    logged_in: true,
    email: active.account_email,
    account_id: active.profile_ref,
    is_bound: true,
  };
}

export const browserPreviewApi = {
  bootstrap(): BootstrapState {
    return {
      overview: buildOverview(),
      accounts: clone(state.accounts),
      settings: clone(state.settings),
    };
  },
  listAccounts(): Account[] {
    return clone(state.accounts);
  },
  listCredentialProfiles(): CredentialProfile[] {
    const officialProfiles = state.accounts.map(profileFromAccount);
    const keyProfiles = state.credentialProfiles.filter((profile) => profile.profile_kind === "third_party_key");
    state.credentialProfiles = [...officialProfiles, ...keyProfiles];
    return clone(state.credentialProfiles);
  },
  createKeyProfile(input: CreateKeyProfileInput): CredentialProfile {
    const apiKey = input.api_key.trim();
    const profile: CredentialProfile = {
      id: state.nextCredentialProfileId++,
      profile_kind: "third_party_key",
      provider: input.provider.trim(),
      nickname: input.nickname.trim(),
      status: "unknown",
      is_active: false,
      base_url: input.base_url.trim(),
      model: input.model.trim(),
      masked_secret: apiKey.length > 8 ? `${apiKey.slice(0, 4)}...${apiKey.slice(-4)}` : "****",
      secret_ref: `keychain://preview-key-${Date.now()}`,
      linked_account_id: null,
      usage_provider_type: input.usage_provider_type,
      usage_query_user: input.usage_query_user.trim() || null,
      usage_query_app_version: input.usage_query_app_version.trim() || null,
      usage_masked_secret: input.usage_access_token.trim()
        ? (input.usage_access_token.trim().length > 8
          ? `${input.usage_access_token.trim().slice(0, 4)}...${input.usage_access_token.trim().slice(-4)}`
          : "****")
        : null,
      usage_summary: input.usage_provider_type === "sub2api" ? previewKeyUsageSummary() : null,
    };
    state.credentialProfiles.push(profile);
    return clone(profile);
  },
  updateKeyProfile(input: UpdateKeyProfileInput): CredentialProfile {
    const profile = state.credentialProfiles.find((item) => item.id === input.id);
    if (!profile || profile.profile_kind !== "third_party_key") {
      throw new Error(`未找到第三方 key #${input.id}`);
    }
    const apiKey = input.api_key.trim();
    profile.provider = input.provider.trim();
    profile.nickname = input.nickname.trim();
    profile.base_url = input.base_url.trim();
    profile.model = input.model.trim();
    if (apiKey) {
      profile.masked_secret = apiKey.length > 8 ? `${apiKey.slice(0, 4)}...${apiKey.slice(-4)}` : "****";
    }
    return clone(profile);
  },
  updateKeyProfileUsageConfig(input: UpdateKeyProfileUsageConfigInput): CredentialProfile {
    const profile = state.credentialProfiles.find((item) => item.id === input.profile_id);
    if (!profile || profile.profile_kind !== "third_party_key") {
      throw new Error(`未找到第三方 key #${input.profile_id}`);
    }
    profile.usage_provider_type = input.usage_provider_type;
    profile.usage_query_user = input.usage_query_user.trim() || null;
    profile.usage_query_app_version = input.usage_query_app_version.trim() || null;
    if (input.usage_access_token.trim()) {
      profile.usage_masked_secret = input.usage_access_token.trim().length > 8
        ? `${input.usage_access_token.trim().slice(0, 4)}...${input.usage_access_token.trim().slice(-4)}`
        : "****";
    }
    profile.usage_summary = input.usage_provider_type === "sub2api" ? previewKeyUsageSummary() : null;
    return clone(profile);
  },
  getKeyProfileUsage(profileId: number): ThirdPartyKeyUsageSummary | null {
    const profile = state.credentialProfiles.find((item) => item.id === profileId);
    if (!profile || profile.profile_kind !== "third_party_key") {
      throw new Error(`未找到第三方 key #${profileId}`);
    }
    return clone(profile.usage_summary ?? null);
  },
  activateCredentialProfile(profileId: number): CredentialProfile {
    const profile = state.credentialProfiles.find((item) => item.id === profileId);
    if (!profile) {
      throw new Error(`未找到凭证身份 #${profileId}`);
    }
    state.credentialProfiles.forEach((item) => {
      item.is_active = item.id === profileId;
    });
    state.accounts.forEach((account) => {
      account.is_active = profile.profile_kind === "official_account" && account.id === profile.linked_account_id;
    });
    return clone(profile);
  },
  getAccountDetail(id: number): AccountDetail {
    return buildAccountDetail(id);
  },
  startCodexLoginFlow(): string {
    return "浏览器预览模式：这里会打开官方登录流程，当前已改为本地预览提示。";
  },
  bindCurrentCodexAccount(input: BindCurrentCodexAccountInput): Account {
    const existing = state.accounts.find((account) => account.account_email === state.currentLogin.email);
    if (existing) {
      return clone(existing);
    }

    const nextId = state.nextAccountId++;
    const newAccount: Account = {
      id: nextId,
      provider: "Codex",
      nickname: input.nickname || "新账号",
      status: "healthy",
      is_active: false,
      is_default: false,
      auth_state: "valid",
      last_check_time: sampleTime("10:45:00"),
      estimated_reset_time: "2026-04-21 15:30:00",
      account_key: `codex-preview-${nextId}`,
      binding_kind: "codex_cli",
      session_ref: `keychain://codex-preview-${nextId}`,
      profile_ref: state.currentLogin.account_id,
      account_email: state.currentLogin.email,
      last_verified_at: sampleTime("10:45:00"),
      is_real_session: true,
      plan_label: null,
      latest_snapshot: makeSnapshot(nextId, "10:45:00", 12, 8, "healthy", "2026-04-21 15:30:00", "2026-04-27 18:00:00"),
    };

    state.accounts.push(newAccount);
    state.currentLogin.is_bound = true;
    return clone(newAccount);
  },
  diagnoseBindEnvironment(): BindEnvironmentDiagnostic {
    return {
      codex_config_dir: "~/.codex",
      auth_path: "~/.codex/auth.json",
      auth_exists: true,
      current_email: state.currentLogin.email,
      current_account_id: state.currentLogin.account_id,
      current_is_bound: state.currentLogin.is_bound,
      cli_candidates: ["/usr/local/bin/codex", "/opt/homebrew/bin/codex"],
      cli_available: "/usr/local/bin/codex",
      cli_status_ok: true,
      cli_stdout: "preview mode",
      cli_stderr: "",
    };
  },
  verifyBoundAccount(id: number): Account {
    const account = state.accounts.find((item) => item.id === id);
    if (!account) {
      throw new Error(`未找到账号 #${id}`);
    }
    account.auth_state = "valid";
    account.status = account.latest_snapshot?.window_5h_percent && account.latest_snapshot.window_5h_percent > 80 ? "warning" : "healthy";
    account.last_verified_at = sampleTime("10:46:00");
    return clone(account);
  },
  deleteAccount(id: number): void {
    state.accounts = state.accounts.filter((account) => account.id !== id);
    if (!activeAccount() && state.accounts[0]) {
      state.accounts[0].is_active = true;
      syncLoginFromActive();
    }
  },
  setDefaultAccount(id: number): Account {
    state.accounts = state.accounts.map((account) => ({ ...account, is_default: account.id === id }));
    const account = state.accounts.find((item) => item.id === id);
    if (!account) {
      throw new Error(`未找到账号 #${id}`);
    }
    return clone(account);
  },
  repairAccountAuth(id: number): Account {
    return this.verifyBoundAccount(id);
  },
  getOverview(): DashboardOverview {
    return buildOverview();
  },
  getWorkspaceSupportData(): WorkspaceSupportData {
    return {
      projects: clone(state.localProjects),
      sessions: clone(state.sessionRecords),
      notifications: clone(state.notifications),
    };
  },
  triggerSampling(): DashboardOverview {
    const active = activeAccount();
    if (active?.latest_snapshot) {
      active.latest_snapshot.sample_time = sampleTime("10:47:00");
      active.latest_snapshot.window_5h_percent = Math.min(99, active.latest_snapshot.window_5h_percent + 2);
      active.latest_snapshot.window_7d_percent = Math.min(99, active.latest_snapshot.window_7d_percent + 1);
    }
    state.latestSampling = {
      kind: "real_updated",
      message: `已刷新 ${active?.nickname ?? "当前账号"} 的预览采样结果。`,
      source_type: "real_usage",
    };
    return buildOverview();
  },
  switchAccount(targetAccountId: number): DashboardOverview {
    const target = state.accounts.find((account) => account.id === targetAccountId);
    const previous = activeAccount();
    if (!target) {
      throw new Error(`未找到账号 #${targetAccountId}`);
    }

    state.accounts.forEach((account) => {
      account.is_active = account.id === targetAccountId;
    });
    syncLoginFromActive();
    target.last_check_time = sampleTime("10:48:00");
    if (target.latest_snapshot) {
      target.latest_snapshot.sample_time = sampleTime("10:48:00");
    }

    state.switchLogs.unshift({
      id: state.nextSwitchId++,
      from_account_id: previous?.id ?? null,
      to_account_id: targetAccountId,
      result: "success",
      reason: `${target.nickname} 当前容量更健康，切换后继续工作。`,
      created_at: sampleTime("10:48:00"),
    });

    state.notifications.unshift({
      id: state.nextNotificationId++,
      account_id: targetAccountId,
      level: "success",
      title: `已切换到 ${target.nickname}`,
      message: "浏览器预览模式下已同步更新当前账号。",
      source_type: "real_switch",
      action_type: "switch_account",
      related_handoff_id: null,
      created_at: sampleTime("10:48:02"),
    });

    state.latestSampling = {
      kind: "real_updated",
      message: `已切换到 ${target.nickname}，并刷新最新采样。`,
      source_type: "real_usage",
    };

    return buildOverview();
  },
  listLocalProjects(): LocalProject[] {
    return clone(state.localProjects);
  },
  listSessionRecords(): SessionRecord[] {
    return clone(state.sessionRecords);
  },
  listSessionsForProfile(profileKind: string, profileRef: string): SessionRecord[] {
    return clone(
      state.sessionRecords.filter(
        (record) => record.owner_profile_kind === profileKind && record.owner_profile_ref === profileRef,
      ),
    );
  },
  importCodexLocalSessions(): CodexLocalSessionImportResult {
    const now = sampleTime("11:02:00");
    let project = state.localProjects.find((item) => item.workspace_path === "/Users/admin/IdeaProjects/Code-Island");
    if (!project) {
      project = {
        id: nextId(state.localProjects),
        name: "Code-Island",
        workspace_path: "/Users/admin/IdeaProjects/Code-Island",
        git_remote: null,
        last_active_at: now,
        created_at: now,
        updated_at: now,
      };
      state.localProjects.unshift(project);
    }

    const externalSessionMarker = "preview-codex-local-session";
    const existing = state.sessionRecords.find((item) => item.raw_content.includes(externalSessionMarker));
    if (existing) {
      existing.updated_at = now;
      existing.message_count = 12;
    } else {
      state.sessionRecords.unshift({
        id: nextId(state.sessionRecords),
        project_id: project.id,
        project_name: project.name,
        project_path: project.workspace_path,
        owner_account_id: null,
        owner_profile_kind: "local_codex",
        owner_profile_ref: "local",
        record_type: "codex_imported",
        title: "优化 Claude code 上岛消息过滤",
        summary: `Codex 本地会话 · 12 条消息 · ${project.workspace_path}`,
        raw_content: JSON.stringify({ source: "codex_local_session", session_id: externalSessionMarker }),
        message_count: 12,
        source_record_id: null,
        created_at: now,
        updated_at: now,
      });
    }

    state.notifications.unshift({
      id: state.nextNotificationId++,
      account_id: null,
      level: "success",
      title: "已导入本地 Codex 会话",
      message: "预览模式已导入 1 条本地 Codex 会话。",
      source_type: "system",
      action_type: "import_codex_local_sessions",
      related_handoff_id: null,
      created_at: now,
    });

    return {
      scanned_files: 1,
      imported_sessions: existing ? 0 : 1,
      updated_sessions: existing ? 1 : 0,
      skipped_files: 0,
      codex_synced_threads: 1,
      codex_skipped_threads: 0,
      project_count: state.localProjects.length,
      session_count: state.sessionRecords.length,
      message: existing
        ? "已扫描 1 个 Codex 本地 session，新增 0 条，更新 1 条，跳过 0 个；已同步到 Codex 1 条，跳过 0 条。"
        : "已扫描 1 个 Codex 本地 session，新增 1 条，更新 0 条，跳过 0 个；已同步到 Codex 1 条，跳过 0 条。",
    };
  },
  listCodexLocalSessionCandidates(): CodexLocalSessionCandidate[] {
    return [
      {
        candidate_id: "preview-codex-current-key",
        identity_key: "key:5",
        identity_label: "语聊",
        identity_kind_label: "Key",
        project_name: "CodexSwitcher",
        project_path: "/Users/admin/IdeaProjects/CodexSwitcher",
        title: "排查测模型与保存key问题",
        message_count: 88,
        source_path: "/Users/admin/.codex/sessions/preview.jsonl",
        created_at: sampleTime("09:01:44"),
        updated_at: sampleTime("09:05:40"),
        imported_session_id: null,
        imported_owner_profile_kind: null,
        imported_owner_profile_ref: null,
      },
    ];
  },
  importCodexLocalSessionCandidates(candidateIds: string[]): CodexLocalSessionImportResult {
    const now = sampleTime("11:06:00");
    const activeKey = state.credentialProfiles.find((profile) => profile.profile_kind === "third_party_key" && profile.is_active);
    for (const candidateId of candidateIds) {
      let project = state.localProjects.find((item) => item.workspace_path === "/Users/admin/IdeaProjects/CodexSwitcher");
      if (!project) {
        project = {
          id: nextId(state.localProjects),
          name: "CodexSwitcher",
          workspace_path: "/Users/admin/IdeaProjects/CodexSwitcher",
          git_remote: null,
          last_active_at: now,
          created_at: now,
          updated_at: now,
        };
        state.localProjects.unshift(project);
      }
      const existing = state.sessionRecords.find((item) => item.raw_content.includes(candidateId));
      if (!existing) {
        state.sessionRecords.unshift({
          id: nextId(state.sessionRecords),
          project_id: project.id,
          project_name: project.name,
          project_path: project.workspace_path,
          owner_account_id: null,
          owner_profile_kind: "third_party_key",
          owner_profile_ref: activeKey ? `key:${activeKey.id}` : "local",
          record_type: "codex_imported",
          title: "排查测模型与保存key问题",
          summary: `Codex 本地会话 · 88 条消息 · ${project.workspace_path}`,
          raw_content: JSON.stringify({ source: "codex_local_session", session_id: candidateId }),
          message_count: 88,
          source_record_id: null,
          created_at: now,
          updated_at: now,
        });
      }
    }
    return {
      scanned_files: candidateIds.length,
      imported_sessions: candidateIds.length,
      updated_sessions: 0,
      skipped_files: 0,
      codex_synced_threads: candidateIds.length,
      codex_skipped_threads: 0,
      project_count: state.localProjects.length,
      session_count: state.sessionRecords.length,
      message: `已导入 ${candidateIds.length} 条选中的本地 Codex 会话；已同步到 Codex ${candidateIds.length} 条。`,
    };
  },
  listNotifications(): NotificationItem[] {
    return clone(state.notifications);
  },
  getReleaseDiagnostic(): ReleaseDiagnostic {
    return buildReleaseDiagnostic();
  },
  getStartupHealth(): StartupHealth {
    return clone(state.startupHealth);
  },
  previewCleanupDebugData(): CleanupPreview {
    return clone(state.cleanupPreview);
  },
  cleanupDebugData(): CleanupResult {
    const deletedTotal = state.cleanupPreview.old_notification_count;
    state.cleanupPreview = {
      old_handoff_count: 0,
      old_notification_count: 0,
      orphan_handoff_count: 0,
    };
    return {
      ...state.cleanupPreview,
      deleted_total: deletedTotal,
    };
  },
  updateSettings(settings: AppSettings): AppSettings {
    state.settings = clone(settings);
    return clone(state.settings);
  },
  openMainWindow(): void {},
};
