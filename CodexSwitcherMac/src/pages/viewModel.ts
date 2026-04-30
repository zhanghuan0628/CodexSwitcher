import type {
  Account,
  AccountAuthState,
  AccountStatus,
  BindEnvironmentDiagnostic,
  CurrentCodexLogin,
  DashboardOverview,
  NotificationItem,
  NotificationSourceType,
  ReleaseDiagnostic,
  StartupHealth,
  StartupSummary,
  StartupSummaryItem,
  StartupSummaryTone,
} from "../types";

export const authStateText: Record<AccountAuthState, string> = {
  valid: "已校验",
  mismatch: "会话不匹配",
  expired: "已失效",
  unknown: "未知",
};

export const notificationFilters: Array<{ key: "all" | NotificationSourceType; label: string }> = [
  { key: "all", label: "全部" },
  { key: "real_login", label: "登录" },
  { key: "real_binding", label: "绑定" },
  { key: "real_verification", label: "校验" },
  { key: "real_repair", label: "修复" },
  { key: "real_switch", label: "切换" },
  { key: "settings_event", label: "设置" },
  { key: "system", label: "系统" },
];

export const statusText: Record<AccountStatus, string> = {
  healthy: "健康",
  warning: "预警",
  exhausted: "不可用",
  auth_invalid: "登录失效",
  error: "异常",
};

export function usageSourceText(sourceType: DashboardOverview["usage_display"]["source_type"]) {
  if (sourceType === "real_usage") return "真实采样";
  return "未知";
}

export function usageSourceTone(sourceType: DashboardOverview["usage_display"]["source_type"]) {
  if (sourceType === "real_usage") return "healthy";
  return "neutral";
}

export function chartSeriesTone(index: number) {
  return ["green", "blue", "neutral", "gold"][index] ?? "neutral";
}

export function hasAuthIssue(account: Account) {
  return account.auth_state !== "valid" || account.status === "auth_invalid";
}

export function switchButtonText(account: Account) {
  if (account.is_active) {
    return "当前账号";
  }
  if (account.auth_state === "valid" && (account.status === "healthy" || account.status === "warning")) {
    return "切换并采样";
  }
  if (account.auth_state === "mismatch") return "重新绑定";
  if (account.auth_state === "expired") return "重新登录";
  if (account.auth_state === "unknown") return "先校验登录态";
  if (account.status === "exhausted") return "等待恢复";
  if (account.status === "auth_invalid") return "修复授权";
  if (account.status === "error") return "排查异常";
  return "不可切换";
}

export function switchHintText(account: Account) {
  if (account.is_active) {
    return "当前活跃账号如需确认登录态，可使用校验操作。";
  }
  if (account.auth_state === "valid" && (account.status === "healthy" || account.status === "warning")) {
    return "可切换账号：点击后会自动切换并刷新真实用量。";
  }
  if (account.auth_state === "mismatch") {
    return "需要重绑：请先登录这张卡对应账号，再执行重新绑定。";
  }
  if (account.auth_state === "expired") {
    return "登录已失效：请重新登录后再切换。";
  }
  if (account.auth_state === "unknown") {
    return "校验前需要先登录这张卡对应账号。";
  }
  if (account.status === "exhausted") {
    return "额度窗口暂不可用：等待恢复后再切换，或选择其他健康账号。";
  }
  return hasAuthIssue(account)
    ? "当前真实账号校验异常，请先重新校验或重新绑定。"
    : "当前真实账号已通过校验，可执行切换并自动采样。";
}

export function accountModeLabel(account: Account) {
  return account.is_real_session ? "真实绑定账号" : "未支持账号";
}

export function accountEmailLabel(account?: Account | null) {
  if (!account) return "未读取到邮箱";
  return account.account_email || "未读取到邮箱";
}

export function accountOfficialIdLabel(account?: Account | null) {
  if (!account) return "--";
  return account.profile_ref || account.account_key || "--";
}

export function accountSnapshot(account?: Account | null) {
  return account?.latest_snapshot ?? null;
}

export function accountUsagePercent(account?: Account | null, key: "window_5h_percent" | "window_7d_percent" = "window_5h_percent") {
  const snapshot = accountSnapshot(account);
  if (!snapshot) return "--";
  return `${snapshot[key]}%`;
}

export function accountResetTime(account: Account | null | undefined, key: "estimated_reset_5h_at" | "estimated_reset_7d_at") {
  const snapshot = accountSnapshot(account);
  if (!snapshot) {
    return account?.is_real_session ? "未知" : "待采样";
  }
  return snapshot[key] ?? "未知";
}

export function accountStatusSummary(account: Account) {
  if (account.auth_state === "valid" && account.status === "healthy") {
    return account.is_active ? "当前账号已就绪，可直接采样。" : "已就绪，可切换并自动采样。";
  }
  if (account.auth_state === "valid" && account.status === "warning") {
    return account.is_active ? "当前账号处于预警区间，建议关注恢复时间。" : "可切换，但处于预警区间。";
  }
  if (account.auth_state === "mismatch") {
    return "绑定会话与当前官方登录态不一致，需要重新绑定。";
  }
  if (account.auth_state === "expired") {
    return "当前真实账号登录已失效，需要重新登录。";
  }
  if (account.auth_state === "unknown") {
    return "当前真实账号尚未确认有效性，建议先执行一次验证。";
  }
  if (account.status === "exhausted") {
    return "当前账号处于耗尽或恢复窗口，暂不建议切换到该账号。";
  }
  if (account.status === "error") {
    return "当前账号状态异常，建议先排查或重新绑定。";
  }
  return "当前真实账号状态需要进一步确认。";
}

export function accountStatusCompactText(account: Account) {
  if (account.auth_state === "valid" && account.status === "healthy") {
    return account.is_active ? "已就绪" : "可切换";
  }
  if (account.auth_state === "valid" && account.status === "warning") {
    return account.is_active ? "当前预警" : "可切换，预警中";
  }
  if (account.auth_state === "mismatch") {
    return "需重绑";
  }
  if (account.auth_state === "expired") {
    return "需重登";
  }
  if (account.auth_state === "unknown") {
    return "待校验";
  }
  if (account.status === "exhausted") {
    return "恢复中";
  }
  if (account.status === "error") {
    return "需排查";
  }
  return "待确认";
}

export function recommendationReasonText(account: Account | null, overviewReason: string | null | undefined, recommendationList: string[]) {
  if (overviewReason) return overviewReason;
  if (recommendationList[0]) return recommendationList[0];
  if (!account) return "这里会显示下一张更合适的账号。";
  if (account.auth_state === "valid" && account.status === "healthy") return "当前可直接切换并采样。";
  if (account.auth_state === "valid" && account.status === "warning") return "当前可切换，但已进入预警区间。";
  return accountStatusSummary(account);
}

export function timelineNextActionLabel(nextAction: string) {
  return nextAction.replace(/^下一步：?/, "").trim();
}

export function dashboardAutoSampleText(status: string) {
  if (!status || status === "自动采样状态未知") return "";
  if (status.includes("已开启") || status.includes("进行中") || status.includes("最近")) return status;
  return "";
}

export function startupSummaryDetailText(detail: string) {
  return detail.length > 36 ? `${detail.slice(0, 36)}…` : detail;
}

export function accountMetaLine(account: Account) {
  return `${account.is_default ? "默认账号 · " : ""}${account.account_email ?? account.profile_ref ?? "未读取到邮箱"}`;
}

export function accountPlanBadgeLabel(account?: Account | null) {
  return account?.plan_label ?? null;
}

export function accountSidebarSummary(account: Account | null) {
  if (!account) return "暂无活跃账号";
  return `${account.nickname} · ${accountStatusCompactText(account)}`;
}

export function accountUsagePair(account: Account | null) {
  return `5h ${accountUsagePercent(account, "window_5h_percent")} · 7d ${accountUsagePercent(account, "window_7d_percent")}`;
}

export function currentLoginMatchesAccount(currentLogin: CurrentCodexLogin | null, account: Account) {
  if (!currentLogin?.logged_in) return false;
  return (
    Boolean(currentLogin.email && account.account_email === currentLogin.email)
    || Boolean(currentLogin.account_id && account.profile_ref === currentLogin.account_id)
  );
}

export function currentLoginIsBound(currentLogin: CurrentCodexLogin | null, accounts: Account[]) {
  if (!currentLogin?.logged_in) return false;
  if (currentLogin.is_bound) return true;
  return accounts.some((account) => currentLoginMatchesAccount(currentLogin, account));
}

export function buildLoginGuideSteps({
  activeAccount,
  bindDiagnostic,
  currentLogin,
  currentLoginIsBound,
  realAccounts,
}: {
  activeAccount: Account | null;
  bindDiagnostic: BindEnvironmentDiagnostic | null;
  currentLogin: CurrentCodexLogin | null;
  currentLoginIsBound: boolean;
  realAccounts: Account[];
}) {
  return [
    {
      title: "1. 检查环境",
      done: Boolean(bindDiagnostic?.auth_exists || currentLogin?.logged_in),
      detail: bindDiagnostic ? `CLI：${bindDiagnostic.cli_available ?? "未找到"}` : "点击“诊断绑定环境”检查 Codex CLI 与官方登录文件。",
    },
    {
      title: "2. 打开官方登录",
      done: Boolean(currentLogin?.logged_in),
      detail: currentLogin?.logged_in ? "已检测到 Codex 官方登录态。" : "点击“开始官方登录”，完成浏览器里的 Codex 登录。",
    },
    {
      title: "3. 自动识别身份",
      done: Boolean(currentLogin?.email || currentLogin?.account_id),
      detail: currentLogin?.email ?? currentLogin?.account_id ?? "等待识别邮箱和官方账号 ID。",
    },
    {
      title: "4. 确认绑定",
      done: currentLoginIsBound,
      detail: currentLoginIsBound ? "当前官方登录态已在账号列表中。" : "确认昵称后点击“绑定当前已登录账号”。",
    },
    {
      title: "5. 立即采样验证",
      done: Boolean(activeAccount?.latest_snapshot),
      detail: activeAccount?.latest_snapshot ? `最近采样：${activeAccount.latest_snapshot.sample_time}` : "绑定后点击“立即采样”验证真实额度读取。",
    },
    {
      title: "6. 开始真实切换",
      done: realAccounts.length >= 2,
      detail: realAccounts.length >= 2 ? "已有多个真实账号，可进入真实切换。" : "至少绑定两个账号后，切换体验会完整生效。",
    },
  ];
}

export function filterNotifications({
  notificationAccountFilter,
  notificationFilter,
  notifications,
  relatedAccountForNotification,
}: {
  notificationAccountFilter: number | "all";
  notificationFilter: "all" | NotificationSourceType;
  notifications: NotificationItem[];
  relatedAccountForNotification: (item: NotificationItem) => Account | null;
}) {
  return notifications.filter((item) => {
    const sourceMatched = notificationFilter === "all" || item.source_type === notificationFilter;
    if (!sourceMatched) return false;
    if (notificationAccountFilter === "all") return true;
    return relatedAccountForNotification(item)?.id === notificationAccountFilter;
  });
}

export function buildSettingsSummaryText(settings: { foreground_auto_sampling_only: boolean }) {
  return !settings.foreground_auto_sampling_only;
}

export function buildUsageSummaryText(usageDisplay: DashboardOverview["usage_display"] | null) {
  return usageDisplay?.summary ?? "等待账号初始化";
}

export function currentLoginLabel(currentLogin: CurrentCodexLogin | null) {
  if (!currentLogin?.logged_in) return "未登录";
  return currentLogin.email ?? currentLogin.account_id ?? "未知官方登录态";
}

export function accountExpectedLoginLabel(account: Account) {
  return account.account_email ?? account.profile_ref ?? account.nickname;
}

export function verifyButtonText(currentLogin: CurrentCodexLogin | null, account: Account) {
  if (!currentLogin?.logged_in) return "先登录再校验";
  if (!currentLoginMatchesAccount(currentLogin, account)) return "需先登录该账号";
  return "校验当前登录态";
}

export function accountSwitchabilitySummary(account: Account) {
  if (account.is_active) return "当前活跃账号：无需切换，可直接采样。";
  if (account.auth_state === "valid" && (account.status === "healthy" || account.status === "warning")) {
    return "可切换：会自动切换并刷新真实用量。";
  }
  if (account.auth_state === "mismatch") return "不可切换：绑定会话与当前官方登录态不一致，需要重新绑定。";
  if (account.auth_state === "expired") return "不可切换：登录已失效，需要重新登录并重绑。";
  if (account.status === "exhausted") return `不可切换：额度窗口暂不可用，5h 预计恢复 ${accountResetTime(account, "estimated_reset_5h_at")}。`;
  if (account.status === "auth_invalid") return "不可切换：授权状态失效，需要修复授权。";
  return "不可切换：账号状态仍需确认。";
}

export function accountSortRank(account: Account) {
  if (account.is_active) return 0;
  if (account.auth_state === "valid" && (account.status === "healthy" || account.status === "warning")) return 1;
  if (hasAuthIssue(account) || account.status === "error") return 2;
  if (account.status === "exhausted") return 3;
  return 4;
}

export function compareAccounts(left: Account, right: Account) {
  const rankDelta = accountSortRank(left) - accountSortRank(right);
  if (rankDelta !== 0) return rankDelta;
  if (left.is_default !== right.is_default) return left.is_default ? -1 : 1;
  return left.nickname.localeCompare(right.nickname, "zh-Hans-CN");
}

export function diagnosticAdvice(errorText: string | null) {
  if (!errorText) return "当前没有新的失败信息。若遇到采样或绑定问题，可以先点“诊断绑定环境”。";
  if (errorText.includes("Keychain") || errorText.includes("keychain") || errorText.includes("item could not be found")) {
    return "Keychain 中找不到这张账号的会话凭证。建议点击该账号的“重新登录并重绑”，让系统重新写入安全凭证。";
  }
  if (errorText.includes("不是") || errorText.includes("不一致") || errorText.includes("mismatch")) {
    return "当前官方登录态与目标账号不一致。请先通过官方登录页切到目标邮箱，再回到 App 等待自动重绑。";
  }
  if (errorText.includes("codex") || errorText.includes("CLI") || errorText.includes("command not found")) {
    return "Codex 官方 CLI 或登录状态不可用。建议先点“开始官方登录”，再点“诊断绑定环境”确认 CLI 状态。";
  }
  if (errorText.includes("额度") || errorText.includes("采样")) {
    return "真实登录态可能有效，但额度读取暂不可用。可以稍后重试采样，或切到账号中心查看登录态诊断。";
  }
  return "建议先刷新状态，再根据账号卡上的“操作建议”选择重新登录、重绑或切换。";
}

export function friendlyErrorText(error: unknown) {
  const detail = String(error);
  if (detail.includes("Keychain") || detail.includes("keychain") || detail.includes("item could not be found")) {
    return `Keychain 中找不到该账号会话凭证，请使用“重新登录并重绑”重新写入凭证。原始信息：${detail}`;
  }
  if (detail.includes("不是") || detail.includes("不一致") || detail.includes("mismatch")) {
    return `当前官方登录态与目标账号不一致，请先登录目标邮箱后再重试。原始信息：${detail}`;
  }
  if (detail.includes("command not found") || detail.includes("codex") || detail.includes("CLI")) {
    return `Codex 官方 CLI 或登录态不可用，请先完成官方登录并刷新状态。原始信息：${detail}`;
  }
  if (detail.includes("No such file") || detail.includes("not found")) {
    return `本地文件或凭证路径不存在，请刷新状态后重试。原始信息：${detail}`;
  }
  return detail;
}

export function statusLabel(status: string) {
  return statusText[status as AccountStatus] ?? status;
}

export function notificationSourceText(sourceType: NotificationSourceType) {
  if (sourceType === "real_login") return "真实登录事件";
  if (sourceType === "real_binding") return "真实绑定事件";
  if (sourceType === "real_verification") return "真实校验事件";
  if (sourceType === "real_repair") return "真实修复事件";
  if (sourceType === "real_switch") return "真实切换事件";
  if (sourceType === "settings_event") return "设置事件";
  return "系统事件";
}

export function notificationActionText(item: NotificationItem) {
  if (item.level === "error") return "建议打开账号详情排查登录态、Keychain 与最近采样。";
  if (item.action_type === "switch_sampled") return "切换和自动采样都已完成，可从账号详情查看项目会话。";
  if (item.action_type === "switch_sample_failed") return "账号已切换，但采样失败；建议打开账号详情后重新采样。";
  if (item.action_type === "sample_mismatch") return "当前官方登录态与绑定账号不一致，请先重新登录并重绑。";
  if (item.source_type === "real_switch") return "可查看账号详情中的项目会话与切换记录。";
  if (item.source_type === "real_verification") return "如提示不一致，请先登录目标账号再重新校验。";
  if (item.source_type === "real_repair") return "修复完成后建议立即采样确认真实额度。";
  if (item.source_type === "real_binding") return "绑定完成后建议执行一次校验和采样。";
  return "暂无额外动作，必要时刷新状态。";
}

export function levelClass(level: string) {
  if (level === "success") return "healthy";
  if (level === "warning") return "warning";
  if (level === "error") return "error";
  return "auth_invalid";
}

function startupSummaryToneRank(tone: StartupSummaryTone) {
  if (tone === "error") return 0;
  if (tone === "warning") return 1;
  return 2;
}

function pushStartupSummaryItem(items: StartupSummaryItem[], item: StartupSummaryItem) {
  if (items.some((existing) => existing.label === item.label && existing.detail === item.detail)) {
    return;
  }
  items.push(item);
}

export function buildStartupSummary(startupHealth: StartupHealth | null, releaseDiagnostic: ReleaseDiagnostic | null): StartupSummary | null {
  if (!startupHealth && !releaseDiagnostic) {
    return null;
  }

  const items: StartupSummaryItem[] = [];

  if (!releaseDiagnostic) {
    pushStartupSummaryItem(items, {
      label: "启动诊断尚未完成",
      detail: "正在读取环境、登录态与最近采样。",
      tone: startupHealth?.healthy === false ? "warning" : "healthy",
      action: "stability",
    });
  }

  if (startupHealth?.healthy === false) {
    startupHealth.checks
      .filter((check) => !check.ok)
      .slice(0, 2)
      .forEach((check) => {
        pushStartupSummaryItem(items, {
          label: check.label,
          detail: check.detail,
          tone: "error",
          action: "stability",
        });
      });
  }

  if (releaseDiagnostic) {
    if (!releaseDiagnostic.codex_cli_available) {
      pushStartupSummaryItem(items, {
        label: "Codex CLI 不可用",
        detail: releaseDiagnostic.codex_cli_path ?? "未检测到可用 CLI 路径。",
        tone: "error",
        action: "stability",
      });
    }

    if (!releaseDiagnostic.database_ok) {
      pushStartupSummaryItem(items, {
        label: "本地数据库异常",
        detail: "启动诊断未通过数据库检查。",
        tone: "error",
        action: "stability",
      });
    }

    if (!releaseDiagnostic.current_login?.logged_in) {
      pushStartupSummaryItem(items, {
        label: "当前未检测到官方登录态",
        detail: "请先完成 Codex 官方登录，再执行绑定或切换。",
        tone: "warning",
        action: "accounts",
      });
    }

    if (releaseDiagnostic.latest_sampling.kind !== "real_updated") {
      pushStartupSummaryItem(items, {
        label: "最近采样未拿到真实数据",
        detail: releaseDiagnostic.latest_sampling.message,
        tone: releaseDiagnostic.latest_sampling.kind === "real_unknown" ? "warning" : "healthy",
        action: "stability",
      });
    }

    const credentialRiskAccounts = releaseDiagnostic.accounts.filter(
      (item) => !item.keychain_readable || item.auth_state !== "valid" || item.status === "auth_invalid" || item.status === "error",
    );

    if (credentialRiskAccounts.length) {
      const firstRisk = credentialRiskAccounts[0];
      pushStartupSummaryItem(items, {
        label: `${firstRisk.nickname} 需要处理凭证或登录态`,
        detail: firstRisk.advice,
        tone: !firstRisk.keychain_readable || firstRisk.status === "error" ? "error" : "warning",
        action: "accounts",
      });
    }
  }

  if (!items.length) {
    return {
      title: "启动检查通过",
      summary: "当前环境已就绪，可直接切换。",
      tone: "healthy",
      generated_at: releaseDiagnostic?.generated_at ?? startupHealth?.generated_at ?? null,
      items: [
        {
          label: "当前可以直接使用",
          detail: "需要细看时，再去发布测试页。",
          tone: "healthy",
          action: "stability",
        },
      ],
    };
  }

  const sorted = [...items].sort((left, right) => startupSummaryToneRank(left.tone) - startupSummaryToneRank(right.tone));
  const tone = sorted[0]?.tone ?? "healthy";
  const title = tone === "error" ? "启动检查发现风险" : tone === "warning" ? "启动检查需要关注" : "启动检查通过";
  const summary = tone === "error"
    ? "建议先处理风险，再继续切换。"
    : tone === "warning"
      ? "可继续查看账号，但建议先看这些提醒。"
      : "当前环境已就绪。";

  return {
    title,
    summary,
    tone,
    generated_at: releaseDiagnostic?.generated_at ?? startupHealth?.generated_at ?? null,
    items: sorted.slice(0, 3),
  };
}
