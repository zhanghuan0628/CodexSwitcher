import { startTransition, useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import "./App.css";
import { api } from "./lib/api";
import { AppContent } from "./pages/AppContent";
import {
  activeIdentityAsset,
  buildIdentityAssets,
  dashboardIdentityCandidates as buildDashboardIdentityCandidates,
  identityShellSubtitle,
  recommendedIdentityAsset,
} from "./pages/identityViewModel";
import type { IdentityAsset } from "./pages/identityViewModel";
import { refreshPlanForIdentityAction } from "./pages/identityRefreshPolicy";
import {
  accountExpectedLoginLabel,
  accountSwitchabilitySummary,
  authStateText,
  buildStartupSummary,
  buildSettingsSummaryText,
  compareAccounts,
  currentLoginIsBound,
  currentLoginLabel,
  currentLoginMatchesAccount,
  friendlyErrorText,
  hasAuthIssue,
} from "./pages/viewModel";
import type {
  Account,
  AccountDetail,
  AppSettings,
  BindEnvironmentDiagnostic,
  BootstrapState,
  CleanupPreview,
  CodexLocalSessionCandidate,
  CreateKeyProfileInput,
  CredentialProfile,
  DashboardOverview,
  LocalProject,
  NotificationItem,
  NotificationSourceType,
  ReleaseDiagnostic,
  SessionRecord,
  StartupHealth,
} from "./types";

type PageKey = "dashboard" | "accounts" | "handoff" | "notifications" | "stability" | "settings";
type NotificationFilter = "all" | NotificationSourceType;
type RefreshOverviewOptions = {
  includeSupportingData?: boolean;
  includeSelectedAccountDetail?: boolean;
  ignoreCooldown?: boolean;
};

const AUTO_REFRESH_INTERVAL_MS = 20_000;
const AUTO_REFRESH_COOLDOWN_MS = 4_000;

const hashToPage: Record<string, PageKey> = {
  "#dashboard": "dashboard",
  "#accounts": "accounts",
  "#handoff": "handoff",
  "#notifications": "notifications",
  "#stability": "stability",
  "#settings": "settings",
};

const emptySettings: AppSettings = {
  warn_threshold_low: 70,
  warn_threshold_mid: 85,
  warn_threshold_high: 95,
  check_interval: 60,
  enable_handoff: true,
  prefer_official_upgrade: true,
  enable_auto_refresh: true,
  enable_auto_sampling: true,
  foreground_auto_sampling_only: false,
  launch_at_login: false,
  menu_bar_only: false,
};

const emptyKeyProfileForm: CreateKeyProfileInput = {
  provider: "custom",
  nickname: "",
  base_url: "",
  model: "gpt-5-codex",
  api_key: "",
  usage_provider_type: "none",
  usage_query_user: "",
  usage_query_app_version: "",
  usage_access_token: "",
};

function looksLikeUrl(value: string) {
  const lowered = value.trim().toLowerCase();
  return lowered.startsWith("http://") || lowered.startsWith("https://") || lowered.includes("://");
}

function mergeProfilesWithExistingUsage(
  nextProfiles: CredentialProfile[],
  currentProfiles: CredentialProfile[],
) {
  const usageByProfileId = new Map(
    currentProfiles
      .filter((profile) => profile.profile_kind === "third_party_key" && profile.usage_summary)
      .map((profile) => [profile.id, profile.usage_summary ?? null]),
  );

  return nextProfiles.map((profile) => profile.profile_kind === "third_party_key"
    ? { ...profile, usage_summary: profile.usage_summary ?? usageByProfileId.get(profile.id) ?? null }
    : profile);
}

function App() {
  const [page, setPage] = useState<PageKey>("dashboard");
  const [overview, setOverview] = useState<DashboardOverview | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [settings, setSettings] = useState<AppSettings>(emptySettings);
  const [localProjects, setLocalProjects] = useState<LocalProject[]>([]);
  const [sessionRecords, setSessionRecords] = useState<SessionRecord[]>([]);
  const [codexLocalSessionCandidates, setCodexLocalSessionCandidates] = useState<CodexLocalSessionCandidate[]>([]);
  const [credentialProfiles, setCredentialProfiles] = useState<CredentialProfile[]>([]);
  const [keyProfileDraft, setKeyProfileDraft] = useState<CreateKeyProfileInput>(emptyKeyProfileForm);
  const [editingKeyProfileId, setEditingKeyProfileId] = useState<number | null>(null);
  const [pendingDeleteKeyProfileId, setPendingDeleteKeyProfileId] = useState<number | null>(null);
  const [keyProfileAction, setKeyProfileAction] = useState<{ profileId: number | null; kind: "save" | "update" | "activate" | "delete" } | null>(null);
  const [keyProfileFormFeedback, setKeyProfileFormFeedback] = useState("");
  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [importingCodexSessions, setImportingCodexSessions] = useState(false);
  const [message, setMessage] = useState<string>("");
  const [lastOperationError, setLastOperationError] = useState<string | null>(null);
  const [bindDiagnostic, setBindDiagnostic] = useState<BindEnvironmentDiagnostic | null>(null);
  const [bindingNickname, setBindingNickname] = useState("");
  const [pendingRepairAccountId, setPendingRepairAccountId] = useState<number | null>(null);
  const [selectedAccountId, setSelectedAccountId] = useState<number | null>(null);
  const [selectedAccountDetail, setSelectedAccountDetail] = useState<AccountDetail | null>(null);
  const [notificationFilter, setNotificationFilter] = useState<NotificationFilter>("all");
  const [notificationAccountFilter, setNotificationAccountFilter] = useState<number | "all">("all");
  const [pendingSwitchAccount, setPendingSwitchAccount] = useState<Account | null>(null);
  const [releaseDiagnostic, setReleaseDiagnostic] = useState<ReleaseDiagnostic | null>(null);
  const [startupHealth, setStartupHealth] = useState<StartupHealth | null>(null);
  const [cleanupPreview, setCleanupPreview] = useState<CleanupPreview | null>(null);
  const [autoSampleStatus, setAutoSampleStatus] = useState("自动采样等待初始化");
  const bootstrapReadyRef = useRef(false);
  const keyUsageRequestIdRef = useRef(0);
  const lastOverviewRefreshAtRef = useRef(0);
  const refreshPromiseRef = useRef<Promise<void> | null>(null);
  const queuedRefreshRef = useRef<Required<RefreshOverviewOptions> | null>(null);
  const supportDataPromiseRef = useRef<Promise<{ projectList: LocalProject[]; sessionList: SessionRecord[]; notificationList: NotificationItem[] }> | null>(null);
  const activeAccount = overview?.active_account ?? null;
  const usageDisplay = overview?.usage_display ?? null;
  const currentLogin = overview?.current_login ?? null;
  const switchLogs = overview?.switch_logs ?? [];

  useEffect(() => {
    void loadBootstrap();
  }, []);

  const autoRefreshOverview = useEffectEvent(() => {
    if (!settings.enable_auto_refresh || !bootstrapReadyRef.current || loading || submitting) {
      return;
    }

    if (document.visibilityState !== "visible") {
      return;
    }

    void (async () => {
      await refreshOverview({
        includeSupportingData: false,
        includeSelectedAccountDetail: false,
        ignoreCooldown: false,
      });
    })();
  });

  useEffect(() => {
    if (!settings.enable_auto_refresh) {
      return;
    }

    const refreshIfVisible = () => {
      autoRefreshOverview();
    };

    document.addEventListener("visibilitychange", refreshIfVisible);
    const timer = window.setInterval(refreshIfVisible, AUTO_REFRESH_INTERVAL_MS);

    return () => {
      document.removeEventListener("visibilitychange", refreshIfVisible);
      window.clearInterval(timer);
    };
  }, [settings.enable_auto_refresh]);

  useEffect(() => {
    const syncPageFromHash = () => {
      const nextPage = hashToPage[window.location.hash];
      if (nextPage) {
        setPage(nextPage);
      }
    };

    syncPageFromHash();
    window.addEventListener("hashchange", syncPageFromHash);
    return () => window.removeEventListener("hashchange", syncPageFromHash);
  }, []);

  const sortedAccounts = useMemo(() => [...accounts].sort(compareAccounts), [accounts]);
  const realAccounts = useMemo(() => sortedAccounts.filter((account) => account.is_real_session), [sortedAccounts]);
  const pendingRepairAccount = useMemo(
    () => accounts.find((account) => account.id === pendingRepairAccountId) ?? null,
    [accounts, pendingRepairAccountId],
  );
  const selectedAccount = useMemo(
    () => accounts.find((account) => account.id === selectedAccountId) ?? null,
    [accounts, selectedAccountId],
  );
  const selectedAccountSessions = useMemo(
    () => selectedAccount ? sessionRecords.filter((record) => record.owner_account_id === selectedAccount.id).slice(0, 5) : [],
    [sessionRecords, selectedAccount],
  );
  const selectedAccountNotifications = useMemo(
    () => selectedAccount
      ? notifications.filter((item) => accountRelatedNotification(item)?.id === selectedAccount.id).slice(0, 5)
      : [],
    [notifications, selectedAccount, accounts],
  );
  const currentLoginBound = useMemo(
    () => currentLoginIsBound(currentLogin, accounts),
    [accounts, currentLogin],
  );

  const continueSamplingWhenHidden = useMemo(
    () => buildSettingsSummaryText(settings),
    [settings],
  );
  const recommendedSwitchAccount = useMemo(
    () => {
      if (overview?.recommended_account_id) {
        return realAccounts.find((account) => account.id === overview.recommended_account_id) ?? null;
      }
      return null;
    },
    [overview?.recommended_account_id, realAccounts],
  );
  const identityAssets = useMemo(
    () => buildIdentityAssets({
      accounts: sortedAccounts,
      credentialProfiles,
      recommendedAccountId: overview?.recommended_account_id,
    }),
    [credentialProfiles, overview?.recommended_account_id, sortedAccounts],
  );
  const activeIdentity = useMemo(
    () => activeIdentityAsset(identityAssets),
    [identityAssets],
  );
  const recommendedIdentity = useMemo(
    () => recommendedIdentityAsset({
      assets: identityAssets,
      recommendedAccountId: overview?.recommended_account_id,
    }),
    [identityAssets, overview?.recommended_account_id],
  );
  const dashboardIdentityAssets = useMemo(
    () => buildDashboardIdentityCandidates({
      assets: identityAssets,
      activeIdentity,
      recommendedIdentity,
      canSwitch,
    }),
    [activeIdentity, identityAssets, recommendedIdentity],
  );
  const currentIdentitySubtitle = useMemo(
    () => identityShellSubtitle(activeIdentity),
    [activeIdentity],
  );
  const recommendationList = overview?.recommendations ?? [];
  const startupSummary = useMemo(
    () => buildStartupSummary(startupHealth, releaseDiagnostic),
    [releaseDiagnostic, startupHealth],
  );

  useEffect(() => {
    if (!currentLogin?.logged_in) {
      return;
    }

    const suggestedName = currentLogin.email ?? currentLogin.account_id ?? "";
    if (!suggestedName) return;

    setBindingNickname(suggestedName);
  }, [currentLogin?.account_id, currentLogin?.email, currentLogin?.logged_in]);

  useEffect(() => {
    if (!settings.enable_auto_sampling) {
      setAutoSampleStatus("自动采样已关闭");
      return;
    }

    if (overview?.latest_sampling?.message) {
      setAutoSampleStatus(overview.latest_sampling.message);
      return;
    }

    setAutoSampleStatus(
      settings.foreground_auto_sampling_only
        ? "自动采样仅在窗口可见时运行"
        : "自动采样已开启，关闭窗口后会继续运行",
    );
  }, [overview?.latest_sampling?.message, settings.enable_auto_sampling, settings.foreground_auto_sampling_only]);

  useEffect(() => {
    if (!pendingRepairAccountId || !pendingRepairAccount || submitting) {
      return;
    }

    if (!currentLogin?.logged_in) {
      setMessage(`等待完成官方登录：请登录 ${accountExpectedLoginLabel(pendingRepairAccount)}。`);
      return;
    }

    if (!currentLoginMatchesAccount(currentLogin, pendingRepairAccount)) {
      setMessage(
        `等待目标账号登录态：当前是 ${currentLoginLabel(currentLogin)}，请在官方登录页切换到 ${accountExpectedLoginLabel(pendingRepairAccount)}。`,
      );
      return;
    }

    let cancelled = false;

    const finishPendingRepair = async () => {
      setSubmitting(true);
      setLastOperationError(null);
      setMessage(`检测到目标账号登录态，正在自动重新绑定 ${pendingRepairAccount.nickname}...`);
      try {
        await api.repairAccountAuth(pendingRepairAccountId);
        if (cancelled) return;
        await refreshOverview();
        if (cancelled) return;
        setPendingRepairAccountId(null);
        setMessage(`${pendingRepairAccount.nickname} 已按当前官方登录态重新绑定`);
      } catch (error) {
        if (cancelled) return;
        const detail = String(error);
        setLastOperationError(detail);
        setMessage(`自动重新绑定失败：${detail}`);
      } finally {
        if (!cancelled) {
          setSubmitting(false);
        }
      }
    };

    void finishPendingRepair();

    return () => {
      cancelled = true;
    };
  }, [
    currentLogin?.account_id,
    currentLogin?.email,
    currentLogin?.logged_in,
    pendingRepairAccount,
    pendingRepairAccountId,
    submitting,
  ]);

  async function loadBootstrap() {
    bootstrapReadyRef.current = false;
    setLoading(true);
    setLastOperationError(null);
    try {
      const [data, profiles] = await Promise.all([
        api.bootstrap(),
        api.listCredentialProfiles(),
      ]);
      applyBootstrap(data);
      const mergedProfiles = mergeProfilesWithExistingUsage(profiles, credentialProfiles);
      setCredentialProfiles(mergedProfiles);
      void refreshKeyUsageForProfiles(mergedProfiles).catch(() => undefined);
      bootstrapReadyRef.current = true;
      setLoading(false);
      void loadSupportingData({ showError: false }).catch(() => undefined);
      setMessage("");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`初始化失败：${detail}`);
      setLoading(false);
    }
  }

  async function loadSupportingData(options: { showError?: boolean } = {}) {
    if (supportDataPromiseRef.current) {
      return supportDataPromiseRef.current;
    }

    const request = (async () => {
      try {
        const [data, candidates] = await Promise.all([
          api.getWorkspaceSupportData(),
          api.listCodexLocalSessionCandidates(),
        ]);
        startTransition(() => {
          setLocalProjects(data.projects);
          setSessionRecords(data.sessions);
          setCodexLocalSessionCandidates(candidates);
          setNotifications(data.notifications);
        });
        return {
          projectList: data.projects,
          sessionList: data.sessions,
          notificationList: data.notifications,
        };
      } catch (error) {
        if (options.showError ?? true) {
          const detail = friendlyErrorText(error);
          setLastOperationError(detail);
        }
        throw error;
      } finally {
        supportDataPromiseRef.current = null;
      }
    })();

    supportDataPromiseRef.current = request;
    return request;
  }

  function applyBootstrap(data: BootstrapState) {
    startTransition(() => {
      setOverview(data.overview);
      setAccounts(data.accounts);
      setSettings(data.settings);
    });
  }

  function applyOverviewData(data: DashboardOverview) {
    startTransition(() => {
      setOverview(data);
      setAccounts(data.accounts);
      setSettings(data.settings);
    });
  }

  async function refreshCredentialProfiles(options: { refreshKeyUsage?: boolean } = {}) {
    const profiles = await api.listCredentialProfiles();
    const mergedProfiles = mergeProfilesWithExistingUsage(profiles, credentialProfiles);
    setCredentialProfiles(mergedProfiles);
    if (options.refreshKeyUsage ?? true) {
      void refreshKeyUsageForProfiles(mergedProfiles).catch(() => undefined);
    }
    return mergedProfiles;
  }

  async function refreshKeyUsageForProfiles(profiles: CredentialProfile[]) {
    const keyProfiles = profiles.filter((profile) => profile.profile_kind === "third_party_key");
    if (!keyProfiles.length) {
      keyUsageRequestIdRef.current += 1;
      return;
    }

    const requestId = ++keyUsageRequestIdRef.current;
    const results = await Promise.all(
      keyProfiles.map(async (profile) => {
        try {
          return [profile.id, await api.getKeyProfileUsage(profile.id)] as const;
        } catch (error) {
          void error;
          return [profile.id, null] as const;
        }
      }),
    );

    if (requestId !== keyUsageRequestIdRef.current) {
      return;
    }

    const usageByProfileId = new Map(results);
    startTransition(() => {
      setCredentialProfiles((currentProfiles) => currentProfiles.map((profile) => {
        if (profile.profile_kind !== "third_party_key") {
          return profile;
        }
        return {
          ...profile,
          usage_summary: usageByProfileId.has(profile.id)
            ? usageByProfileId.get(profile.id) ?? null
            : profile.usage_summary ?? null,
        };
      }));
    });
  }

  function mergeRefreshOptions(
    current: Required<RefreshOverviewOptions> | null,
    next: Required<RefreshOverviewOptions>,
  ): Required<RefreshOverviewOptions> {
    return {
      includeSupportingData: Boolean(current?.includeSupportingData) || next.includeSupportingData,
      includeSelectedAccountDetail:
        Boolean(current?.includeSelectedAccountDetail) || next.includeSelectedAccountDetail,
      ignoreCooldown: Boolean(current?.ignoreCooldown) || next.ignoreCooldown,
    };
  }

  async function refreshOverview(options: RefreshOverviewOptions = {}) {
    const normalized: Required<RefreshOverviewOptions> = {
      includeSupportingData: options.includeSupportingData ?? true,
      includeSelectedAccountDetail: options.includeSelectedAccountDetail ?? Boolean(selectedAccountId),
      ignoreCooldown: options.ignoreCooldown ?? true,
    };

    if (
      !normalized.ignoreCooldown
      && Date.now() - lastOverviewRefreshAtRef.current < AUTO_REFRESH_COOLDOWN_MS
    ) {
      return refreshPromiseRef.current ?? Promise.resolve();
    }

    if (refreshPromiseRef.current) {
      queuedRefreshRef.current = mergeRefreshOptions(queuedRefreshRef.current, normalized);
      return refreshPromiseRef.current;
    }

    const request = (async () => {
      try {
        const data = await api.getOverview();
        applyOverviewData(data);
        lastOverviewRefreshAtRef.current = Date.now();

        if (normalized.includeSupportingData) {
          await loadSupportingData();
        }
        if (normalized.includeSelectedAccountDetail && selectedAccountId) {
          await loadAccountDetail(selectedAccountId);
        }
      } finally {
        refreshPromiseRef.current = null;
        const queuedRefresh = queuedRefreshRef.current;
        queuedRefreshRef.current = null;
        if (queuedRefresh) {
          await refreshOverview({
            ...queuedRefresh,
            ignoreCooldown: true,
          });
        }
      }
    })();

    refreshPromiseRef.current = request;
    return request;
  }

  async function loadAccountDetail(id: number) {
    try {
      const detail = await api.getAccountDetail(id);
      startTransition(() => {
        setSelectedAccountDetail(detail);
        setSelectedAccountId(id);
      });
    } catch (error) {
      setSelectedAccountDetail(null);
      setMessage(`账号详情加载失败：${friendlyErrorText(error)}`);
    }
  }

  async function handleBindCurrentAccount() {
    const nickname = bindingNickname.trim();
    if (!nickname) {
      setMessage("请输入账号昵称");
      return;
    }

    setSubmitting(true);
    setBindDiagnostic(null);
    setLastOperationError(null);
    setMessage("正在绑定当前官方登录态...");
    try {
      await api.bindCurrentCodexAccount({ nickname });
      await refreshOverview();
      await refreshCredentialProfiles();
      setBindingNickname(currentLogin?.email ?? currentLogin?.account_id ?? "");
      setMessage("当前官方登录态已绑定");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`绑定失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function startLoginFlow() {
    setSubmitting(true);
    setMessage("正在打开官方登录流程...");
    try {
      const result = await api.startCodexLoginFlow();
      setMessage(result);
    } catch (error) {
      setMessage(`打开官方登录流程失败：${friendlyErrorText(error)}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function diagnoseBindEnvironment() {
    setSubmitting(true);
    setLastOperationError(null);
    setMessage("正在检查绑定环境...");
    try {
      const diagnostic = await api.diagnoseBindEnvironment();
      setBindDiagnostic(diagnostic);
      setMessage("绑定环境诊断已更新");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`绑定环境诊断失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function loadReleaseDiagnostic() {
    setSubmitting(true);
    setLastOperationError(null);
    try {
      const [diagnostic, preview, health] = await Promise.all([
        api.getReleaseDiagnostic(),
        api.previewCleanupDebugData(),
        api.getStartupHealth(),
      ]);
      startTransition(() => {
        setReleaseDiagnostic(diagnostic);
        setCleanupPreview(preview);
        setStartupHealth(health);
      });
      setMessage("发布前诊断已更新");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`发布前诊断失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function loadStartupHealth(showMessage = true) {
    try {
      const health = await api.getStartupHealth();
      startTransition(() => {
        setStartupHealth(health);
      });
      if (showMessage) {
        setMessage(health.healthy ? "启动健康检查通过" : "启动健康检查发现风险，请查看发布测试页");
      }
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      if (showMessage) {
        setMessage(`启动健康检查失败：${detail}`);
      }
    }
  }

  async function cleanupDebugData() {
    const preview = cleanupPreview ?? await api.previewCleanupDebugData();
    const total = preview.old_notification_count;
    if (total <= 0) {
      setMessage("没有需要清理的历史调试数据");
      setCleanupPreview(preview);
      return;
    }

    const confirmed = window.confirm(
      `将清理 ${total} 条历史调试通知。\n不会删除真实账号、真实采样、项目或会话记录。\n\n确认继续吗？`,
    );
    if (!confirmed) {
      setMessage("已取消清理历史调试数据");
      return;
    }

    setSubmitting(true);
    try {
      const result = await api.cleanupDebugData();
      await refreshOverview();
      const nextPreview = await api.previewCleanupDebugData();
      setCleanupPreview(nextPreview);
      setMessage(`已清理 ${result.deleted_total} 条历史调试数据`);
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`清理失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function removeAccount(id: number) {
    const account = accounts.find((item) => item.id === id);
    if (account?.is_active || (account && currentLoginMatchesAccount(currentLogin, account))) {
      setMessage("当前登录的官方账号不能删除，请先切换到其他身份。");
      return;
    }
    setSubmitting(true);
    try {
      await api.deleteAccount(id);
      await refreshOverview();
      setMessage("账号已删除");
    } catch (error) {
      setMessage(`删除失败：${friendlyErrorText(error)}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function makeDefault(id: number) {
    setSubmitting(true);
    try {
      await api.setDefaultAccount(id);
      await refreshOverview();
      setMessage("默认账号已更新");
    } catch (error) {
      setMessage(`设置默认账号失败：${friendlyErrorText(error)}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function sampleNow() {
    setSubmitting(true);
    setLastOperationError(null);
    try {
      const data = await api.triggerSampling();
      applyOverviewData(data);
      await loadSupportingData();
      await refreshCredentialProfiles();
      setMessage(data.latest_sampling.message);
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`采样失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function refreshStatusNow() {
    const plan = refreshPlanForIdentityAction("refresh-status");
    setSubmitting(true);
    setLastOperationError(null);
    setMessage("正在刷新本地状态...");
    try {
      let samplingMessage: string | null = null;
      if (plan.credentialProfiles) {
        await refreshCredentialProfiles({ refreshKeyUsage: plan.keyUsage });
      }
      if (plan.sampling) {
        const data = await api.triggerSampling();
        applyOverviewData(data);
        samplingMessage = data.latest_sampling.message;
      }
      if (plan.overview) {
        await refreshOverview({
          includeSupportingData: plan.supportingData,
          includeSelectedAccountDetail: false,
          ignoreCooldown: true,
        });
      }
      setMessage(samplingMessage ?? "状态已刷新");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`刷新状态失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function importCodexLocalSessions(candidateIds?: string[]) {
    setImportingCodexSessions(true);
    setLastOperationError(null);
    setMessage(candidateIds?.length ? "正在导入选中的本地 Codex 会话..." : "正在导入本地 Codex 会话...");
    try {
      const result = candidateIds?.length
        ? await api.importCodexLocalSessionCandidates(candidateIds)
        : await api.importCodexLocalSessions();
      await loadSupportingData();
      setMessage(result.message);
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`导入本地 Codex 会话失败：${detail}`);
    } finally {
      setImportingCodexSessions(false);
    }
  }

  async function switchAccount(id: number) {
    const target = accounts.find((account) => account.id === id);
    if (!target) {
      setMessage("切换失败：未找到目标账号");
      return;
    }
    setPendingSwitchAccount(target);
  }

  async function executeSwitchAccount(id: number) {
    const plan = refreshPlanForIdentityAction("switch-official-account");
    setSubmitting(true);
    setLastOperationError(null);
    try {
      const data = await api.switchAccount(id);
      if (plan.overview) {
        applyOverviewData(data);
      }
      if (plan.credentialProfiles) {
        await refreshCredentialProfiles({ refreshKeyUsage: plan.keyUsage });
      }
      if (plan.supportingData) {
        await loadSupportingData();
      }
      setMessage("账号切换成功；状态会在后台马上刷新。");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      await refreshOverview();
      setMessage(`切换失败：${detail}`);
    } finally {
      setSubmitting(false);
      setPendingSwitchAccount(null);
    }
  }

  async function repairAuth(id: number) {
    const target = accounts.find((account) => account.id === id);
    if (target?.is_real_session && !currentLoginMatchesAccount(currentLogin, target)) {
      const detail = `当前官方登录态是 ${currentLoginLabel(currentLogin)}，不是 ${accountExpectedLoginLabel(target)}。请先点击“重新登录并重绑”。`;
      setLastOperationError(detail);
      setMessage(`授权修复已拦截：${detail}`);
      return;
    }

    setSubmitting(true);
    setLastOperationError(null);
    try {
      await api.repairAccountAuth(id);
      await refreshOverview();
      setMessage("授权修复成功");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`授权修复失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function beginRepairFlow(id: number) {
    const target = accounts.find((account) => account.id === id);
    setPendingRepairAccountId(id);
    setSubmitting(true);
    setLastOperationError(null);
    setMessage(`正在打开官方登录流程，请登录 ${target ? accountExpectedLoginLabel(target) : "这张账号卡"}；登录完成后会自动重绑。`);
    try {
      const result = await api.startCodexLoginFlow();
      setMessage(result);
    } catch (error) {
      setPendingRepairAccountId(null);
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`打开官方登录流程失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function verifyAccount(id: number) {
    setSubmitting(true);
    setLastOperationError(null);
    try {
      await api.verifyBoundAccount(id);
      await refreshOverview();
      setMessage("账号校验已完成");
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`账号校验失败：${detail}`);
    } finally {
      setSubmitting(false);
    }
  }

  async function createKeyProfile() {
    const failKeyProfileForm = (feedback: string) => {
      setKeyProfileFormFeedback(feedback);
      setMessage(feedback);
    };

    if (!keyProfileDraft.nickname.trim()) {
      failKeyProfileForm("请输入 key 昵称");
      return;
    }
    if (!keyProfileDraft.base_url.trim()) {
      failKeyProfileForm("请输入 key 的 base URL");
      return;
    }
    if (!keyProfileDraft.model.trim()) {
      failKeyProfileForm("请输入模型名称");
      return;
    }
    if (!editingKeyProfileId && !keyProfileDraft.api_key.trim()) {
      failKeyProfileForm("请输入 API key");
      return;
    }
    if (keyProfileDraft.api_key.trim() && looksLikeUrl(keyProfileDraft.api_key)) {
      failKeyProfileForm("API Key 不能填写 Base URL，请填写供应商后台生成的真实 key");
      return;
    }
    if (keyProfileDraft.usage_access_token.trim() && looksLikeUrl(keyProfileDraft.usage_access_token)) {
      failKeyProfileForm("访问令牌不能填写 URL，请填写 oneTop 个人安全设置里的访问令牌");
      return;
    }
    setSubmitting(true);
    setKeyProfileAction({ profileId: editingKeyProfileId, kind: editingKeyProfileId ? "update" : "save" });
    setKeyProfileFormFeedback("");
    setLastOperationError(null);
    try {
      if (editingKeyProfileId) {
        const updated = await api.updateKeyProfile({ ...keyProfileDraft, id: editingKeyProfileId });
        await api.updateKeyProfileUsageConfig({
          profile_id: updated.id,
          usage_provider_type: keyProfileDraft.usage_provider_type,
          usage_query_user: keyProfileDraft.usage_query_user,
          usage_query_app_version: keyProfileDraft.usage_query_app_version,
          usage_access_token: keyProfileDraft.usage_access_token,
        });
      } else {
        const created = await api.createKeyProfile(keyProfileDraft);
        await api.updateKeyProfileUsageConfig({
          profile_id: created.id,
          usage_provider_type: keyProfileDraft.usage_provider_type,
          usage_query_user: keyProfileDraft.usage_query_user,
          usage_query_app_version: keyProfileDraft.usage_query_app_version,
          usage_access_token: keyProfileDraft.usage_access_token,
        });
      }
      await refreshCredentialProfiles();
      setKeyProfileDraft(emptyKeyProfileForm);
      setEditingKeyProfileId(null);
      const feedback = editingKeyProfileId ? "第三方 key 已更新" : "第三方 key 已保存为账号资产";
      setKeyProfileFormFeedback(feedback);
      setMessage(feedback);
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      const feedback = `${editingKeyProfileId ? "更新" : "保存"} key 失败：${detail}`;
      setKeyProfileFormFeedback(feedback);
      setMessage(feedback);
    } finally {
      setSubmitting(false);
      setKeyProfileAction(null);
    }
  }

  function beginEditKeyProfile(profile: CredentialProfile) {
    setPendingDeleteKeyProfileId(null);
    setEditingKeyProfileId(profile.id);
    setKeyProfileDraft({
      provider: profile.provider,
      nickname: profile.nickname,
      base_url: profile.base_url ?? "",
      model: profile.model ?? "gpt-5-codex",
      api_key: "",
      usage_provider_type: (profile.usage_provider_type as "none" | "sub2api" | "new_api" | null) ?? "none",
      usage_query_user: profile.usage_query_user ?? "",
      usage_query_app_version: profile.usage_query_app_version ?? "",
      usage_access_token: "",
    });
    setKeyProfileFormFeedback("API Key 和访问令牌留空表示不修改原凭证");
    setMessage(`正在编辑 ${profile.nickname}，API Key 和访问令牌留空表示不修改原凭证`);
  }

  function cancelEditKeyProfile() {
    setEditingKeyProfileId(null);
    setPendingDeleteKeyProfileId(null);
    setKeyProfileDraft(emptyKeyProfileForm);
    setKeyProfileFormFeedback("");
    setMessage("已取消编辑 key");
  }

  function updateKeyProfileDraft(value: CreateKeyProfileInput) {
    setKeyProfileDraft(value);
    if (keyProfileFormFeedback) {
      setKeyProfileFormFeedback("");
    }
  }

  async function activateCredentialProfile(profileId: number) {
    const plan = refreshPlanForIdentityAction("activate-third-party-key");
    setSubmitting(true);
    setKeyProfileAction({ profileId, kind: "activate" });
    setLastOperationError(null);
    try {
      const profile = await api.activateCredentialProfile(profileId);
      if (plan.credentialProfiles) {
        await refreshCredentialProfiles({ refreshKeyUsage: plan.keyUsage });
      }
      if (plan.overview) {
        await refreshOverview({
          includeSupportingData: plan.supportingData,
          includeSelectedAccountDetail: false,
          ignoreCooldown: true,
        });
      }
      if (profile.profile_kind === "third_party_key") {
        setMessage(`${profile.nickname} 已启用，并已写入 Codex auth.json/config.toml`);
      } else {
        setMessage(`${profile.nickname} 已设为当前身份`);
      }
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`启用身份失败：${detail}`);
    } finally {
      setSubmitting(false);
      setKeyProfileAction(null);
    }
  }

  async function deleteKeyProfile(profile: CredentialProfile) {
    if (profile.profile_kind !== "third_party_key") {
      setMessage("官方账号资产不能删除。");
      return;
    }
    if (profile.is_active) {
      setMessage("当前登录的 Key 不能删除，请先切换到其他身份。");
      return;
    }

    if (pendingDeleteKeyProfileId !== profile.id) {
      setPendingDeleteKeyProfileId(profile.id);
      setMessage(`再次点击“确认删除”将删除 ${profile.nickname}，并移除本机保存的 Keychain 凭证。`);
      return;
    }

    setSubmitting(true);
    setKeyProfileAction({ profileId: profile.id, kind: "delete" });
    setLastOperationError(null);
    try {
      await api.deleteCredentialProfile(profile.id);
      await refreshCredentialProfiles();
      if (editingKeyProfileId === profile.id) {
        setEditingKeyProfileId(null);
        setKeyProfileDraft(emptyKeyProfileForm);
        setKeyProfileFormFeedback("");
      }
      setMessage(`${profile.nickname} 已删除`);
    } catch (error) {
      const detail = friendlyErrorText(error);
      setLastOperationError(detail);
      setMessage(`删除 Key 失败：${detail}`);
    } finally {
      setSubmitting(false);
      setKeyProfileAction(null);
      setPendingDeleteKeyProfileId(null);
    }
  }

  async function activateIdentity(asset: IdentityAsset) {
    if (asset.kind === "third_party_key") {
      await activateCredentialProfile(asset.profile.id);
      return;
    }
    if (asset.account) {
      await switchAccount(asset.account.id);
    }
  }

  async function saveSettings() {
    setSubmitting(true);
    try {
      const updated = await api.updateSettings(settings);
      setSettings(updated);
      await refreshOverview();
      setMessage("设置已保存");
    } catch (error) {
      setMessage(`设置保存失败：${friendlyErrorText(error)}`);
    } finally {
      setSubmitting(false);
    }
  }

  const regressionChecks = useMemo(() => {
    const hasMultipleAccounts = realAccounts.length >= 2;
    const hasActiveSample = Boolean(activeAccount?.latest_snapshot);
    const hasSwitchLog = switchLogs.some((log) => log.result === "success");
    const hasStructuredSwitchNotification = notifications.some(
      (item) => item.source_type === "real_switch" && item.account_id !== null,
    );
    const allAccountsKeychainOk = releaseDiagnostic
      ? releaseDiagnostic.accounts.every((item) => item.keychain_readable)
      : false;

    return [
      {
        label: "至少绑定 2 个真实 Codex 账号",
        passed: hasMultipleAccounts,
        detail: `当前真实账号数：${realAccounts.length}`,
      },
      {
        label: "当前活跃账号已有真实采样",
        passed: hasActiveSample,
        detail: activeAccount?.latest_snapshot?.sample_time ?? "还没有当前账号真实采样",
      },
      {
        label: "存在真实切换成功记录",
        passed: hasSwitchLog,
        detail: switchLogs.find((log) => log.result === "success")?.created_at ?? "暂无成功切换记录",
      },
      {
        label: "切换通知已结构化关联账号",
        passed: hasStructuredSwitchNotification,
        detail: hasStructuredSwitchNotification ? "real_switch 通知已带 account_id" : "等待下一次真实切换产生结构化通知",
      },
      {
        label: "项目会话库已就绪",
        passed: true,
        detail: `项目 ${localProjects.length} 个，会话 ${sessionRecords.length} 条`,
      },
      {
        label: "Keychain 凭证诊断通过",
        passed: allAccountsKeychainOk,
        detail: releaseDiagnostic ? "诊断已执行" : "请先点击“刷新发布诊断”",
      },
    ];
  }, [activeAccount, localProjects.length, notifications, realAccounts.length, releaseDiagnostic, sessionRecords.length, switchLogs]);

  async function copyAccountDiagnostic() {
    if (!selectedAccountDetail) {
      setMessage("当前没有可复制的账号诊断信息");
      return;
    }

    try {
      await navigator.clipboard.writeText(selectedAccountDetail.diagnostic_text);
      setMessage("账号诊断信息已复制");
    } catch (error) {
      setMessage(`复制诊断信息失败：${String(error)}`);
    }
  }

  function canSwitch(account: Account) {
    return !account.is_active && account.auth_state === "valid" && (account.status === "healthy" || account.status === "warning");
  }

  function openAccountDetail(account: Account) {
    void loadAccountDetail(account.id);
  }

  function accountRelatedNotification(item: NotificationItem) {
    if (item.account_id) {
      return accounts.find((account) => account.id === item.account_id) ?? null;
    }

    const text = `${item.title} ${item.message}`;
    return accounts.find((account) => {
      const candidates = [
        account.nickname,
        account.account_email,
        account.profile_ref,
        account.account_key,
      ].filter(Boolean) as string[];
      return candidates.some((candidate) => text.includes(candidate));
    }) ?? null;
  }


  function navigateTo(nextPage: PageKey) {
    setPage(nextPage);
    window.location.hash = `#${nextPage}`;
  }

  function closeSelectedAccount() {
    setSelectedAccountId(null);
    setSelectedAccountDetail(null);
  }

  return (
    <div className="app-shell">
      <main className="main-panel">
        <AppContent
          activeAccount={activeAccount}
          autoSampleStatus={autoSampleStatus}
          authStateText={authStateText}
          bindDiagnostic={bindDiagnostic}
          bindingNickname={bindingNickname}
          canSwitch={canSwitch}
          cleanupPreview={cleanupPreview}
          continueSamplingWhenHidden={continueSamplingWhenHidden}
          credentialProfiles={credentialProfiles}
          currentLogin={currentLogin}
          currentLoginIsBound={currentLoginBound}
          editingKeyProfileId={editingKeyProfileId}
          localProjects={localProjects}
          sessionRecords={sessionRecords}
          codexLocalSessionCandidates={codexLocalSessionCandidates}
          importingCodexSessions={importingCodexSessions}
          activeIdentity={activeIdentity}
          dashboardIdentityAssets={dashboardIdentityAssets}
          identityAssets={identityAssets}
          recommendedIdentity={recommendedIdentity}
          currentIdentitySubtitle={currentIdentitySubtitle}
          keyProfileDraft={keyProfileDraft}
          keyProfileAction={keyProfileAction}
          keyProfileFormFeedback={keyProfileFormFeedback}
          hasAuthIssue={hasAuthIssue}
          lastOperationError={lastOperationError}
          loading={loading}
          message={message}
          notificationAccountFilter={notificationAccountFilter}
          notificationFilter={notificationFilter}
          notifications={notifications}
          overview={overview}
          page={page}
          pendingDeleteKeyProfileId={pendingDeleteKeyProfileId}
          pendingRepairAccount={pendingRepairAccount}
          pendingSwitchAccount={pendingSwitchAccount}
          realAccounts={realAccounts}
          recommendationList={recommendationList}
          recommendedSwitchAccount={recommendedSwitchAccount}
          regressionChecks={regressionChecks}
          releaseDiagnostic={releaseDiagnostic}
          selectedAccount={selectedAccount}
          selectedAccountDetail={selectedAccountDetail}
          selectedAccountSessions={selectedAccountSessions}
          selectedAccountNotifications={selectedAccountNotifications}
          settings={settings}
          startupHealth={startupHealth}
          startupSummary={startupSummary}
          submitting={submitting}
          switchLogs={switchLogs}
          usageDisplay={usageDisplay}
          accountSwitchabilitySummary={accountSwitchabilitySummary}
          onBeginEditKeyProfile={beginEditKeyProfile}
          onBeginRepairFlow={beginRepairFlow}
          onCancelEditKeyProfile={cancelEditKeyProfile}
          onCleanupDebugData={cleanupDebugData}
          onCloseSelectedAccount={closeSelectedAccount}
          onCloseSwitchConfirm={() => setPendingSwitchAccount(null)}
          onCopyAccountDiagnostic={copyAccountDiagnostic}
          onCreateKeyProfile={createKeyProfile}
          onDeleteKeyProfile={deleteKeyProfile}
          onDiagnoseBindEnvironment={diagnoseBindEnvironment}
          onExecuteSwitchAccount={executeSwitchAccount}
          onImportCodexLocalSessions={importCodexLocalSessions}
          onGoAccounts={() => navigateTo("accounts")}
          onGoHandoff={() => navigateTo("handoff")}
          onGoStability={() => navigateTo("stability")}
          onHandleBindCurrentAccount={handleBindCurrentAccount}
          onLoadAccountDetail={loadAccountDetail}
          onLoadBootstrap={loadBootstrap}
          onLoadReleaseDiagnostic={loadReleaseDiagnostic}
          onLoadStartupHealth={loadStartupHealth}
          onMakeDefault={makeDefault}
          onNavigate={navigateTo}
          onOpenAccountDetail={openAccountDetail}
          onRefreshOverview={refreshStatusNow}
          onRemoveAccount={removeAccount}
          onRepairAuth={repairAuth}
          onSampleNow={sampleNow}
          onSaveSettings={saveSettings}
          onActivateIdentity={activateIdentity}
          onSetBindingNickname={setBindingNickname}
          onSetKeyProfileDraft={updateKeyProfileDraft}
          onSetNotificationAccountFilter={setNotificationAccountFilter}
          onSetNotificationFilter={setNotificationFilter}
          onSetSettings={setSettings}
          onShowSwitchConfirm={switchAccount}
          onStartLoginFlow={startLoginFlow}
          onVerifyAccount={verifyAccount}
          relatedAccountForNotification={accountRelatedNotification}
        />
      </main>
    </div>
  );
}

export default App;
