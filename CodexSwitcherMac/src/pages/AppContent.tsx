import { AppFrame } from "../components/AppFrame";
import { ContextSidebar } from "../components/ContextSidebar";
import { pageTitles } from "../shell/layout";
import { AppPageBody } from "./AppPageBody";
import type {
  Account,
  AccountAuthState,
  AccountDetail,
  AppSettings,
  BindEnvironmentDiagnostic,
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
  StartupSummary,
  SwitchLog,
} from "../types";
import type { IdentityAsset } from "./identityViewModel";
import { identitySummaryText } from "./identityViewModel";
import type { PageKey, SidebarAction, SidebarSectionData } from "../shell/layout";

type NotificationFilter = "all" | NotificationSourceType;

type AppContentProps = {
  activeAccount: Account | null;
  autoSampleStatus: string;
  authStateText: Record<AccountAuthState, string>;
  bindDiagnostic: BindEnvironmentDiagnostic | null;
  bindingNickname: string;
  canSwitch: (account: Account) => boolean;
  cleanupPreview: CleanupPreview | null;
  continueSamplingWhenHidden: boolean;
  credentialProfiles: CredentialProfile[];
  currentLogin: {
    logged_in: boolean;
    email: string | null;
    account_id: string | null;
    is_bound: boolean;
  } | null;
  currentLoginIsBound: boolean;
  editingKeyProfileId: number | null;
  localProjects: LocalProject[];
  sessionRecords: SessionRecord[];
  codexLocalSessionCandidates: CodexLocalSessionCandidate[];
  importingCodexSessions: boolean;
  activeIdentity: IdentityAsset | null;
  currentIdentitySubtitle: string;
  dashboardIdentityAssets: IdentityAsset[];
  identityAssets: IdentityAsset[];
  recommendedIdentity: IdentityAsset | null;
  keyProfileDraft: CreateKeyProfileInput;
  keyProfileAction: { profileId: number | null; kind: "save" | "update" | "activate" } | null;
  keyProfileFormFeedback: string;
  hasAuthIssue: (account: Account) => boolean;
  lastOperationError: string | null;
  loading: boolean;
  message: string;
  notificationAccountFilter: number | "all";
  notificationFilter: NotificationFilter;
  notifications: NotificationItem[];
  overview: DashboardOverview | null;
  page: PageKey;
  pendingRepairAccount: Account | null;
  pendingSwitchAccount: Account | null;
  realAccounts: Account[];
  recommendationList: string[];
  recommendedSwitchAccount: Account | null;
  regressionChecks: Array<{ label: string; passed: boolean; detail: string }>;
  releaseDiagnostic: ReleaseDiagnostic | null;
  selectedAccount: Account | null;
  selectedAccountDetail: AccountDetail | null;
  selectedAccountSessions: SessionRecord[];
  selectedAccountNotifications: NotificationItem[];
  settings: AppSettings;
  startupHealth: StartupHealth | null;
  startupSummary: StartupSummary | null;
  submitting: boolean;
  switchLogs: SwitchLog[];
  usageDisplay: DashboardOverview["usage_display"] | null;
  accountSwitchabilitySummary: (account: Account) => string;
  onBeginEditKeyProfile: (profile: CredentialProfile) => void;
  onBeginRepairFlow: (id: number) => void | Promise<void>;
  onCancelEditKeyProfile: () => void;
  onCleanupDebugData: () => void | Promise<void>;
  onCloseSelectedAccount: () => void;
  onCloseSwitchConfirm: () => void;
  onCopyAccountDiagnostic: () => void | Promise<void>;
  onCreateKeyProfile: () => void | Promise<void>;
  onDiagnoseBindEnvironment: () => void | Promise<void>;
  onExecuteSwitchAccount: (id: number) => void | Promise<void>;
  onImportCodexLocalSessions: (candidateIds?: string[]) => void | Promise<void>;
  onGoAccounts: () => void;
  onGoHandoff: () => void;
  onGoStability: () => void;
  onHandleBindCurrentAccount: () => void | Promise<void>;
  onLoadAccountDetail: (id: number) => void | Promise<void>;
  onLoadBootstrap: () => void | Promise<void>;
  onLoadReleaseDiagnostic: () => void | Promise<void>;
  onLoadStartupHealth: () => void | Promise<void>;
  onMakeDefault: (id: number) => void | Promise<void>;
  onNavigate: (page: PageKey) => void;
  onOpenAccountDetail: (account: Account) => void;
  onRefreshOverview: () => void | Promise<void>;
  onRemoveAccount: (id: number) => void | Promise<void>;
  onRepairAuth: (id: number) => void | Promise<void>;
  onSampleNow: () => void | Promise<void>;
  onSaveSettings: () => void | Promise<void>;
  onActivateIdentity: (asset: IdentityAsset) => void | Promise<void>;
  onSetBindingNickname: (value: string) => void;
  onSetKeyProfileDraft: (value: CreateKeyProfileInput) => void;
  onSetNotificationAccountFilter: (value: number | "all") => void;
  onSetNotificationFilter: (value: NotificationFilter) => void;
  onSetSettings: (settings: AppSettings) => void;
  onShowSwitchConfirm: (id: number) => void | Promise<void>;
  onStartLoginFlow: () => void | Promise<void>;
  onVerifyAccount: (id: number) => void | Promise<void>;
  relatedAccountForNotification: (item: NotificationItem) => Account | null;
};

export function AppContent({
  activeAccount,
  autoSampleStatus,
  authStateText,
  bindDiagnostic,
  bindingNickname,
  canSwitch,
  cleanupPreview,
  continueSamplingWhenHidden,
  credentialProfiles,
  currentLogin,
  currentLoginIsBound,
  editingKeyProfileId,
  localProjects,
  sessionRecords,
  codexLocalSessionCandidates,
  importingCodexSessions,
  activeIdentity,
  currentIdentitySubtitle,
  dashboardIdentityAssets,
  identityAssets,
  recommendedIdentity,
  keyProfileDraft,
  keyProfileAction,
  keyProfileFormFeedback,
  hasAuthIssue,
  lastOperationError,
  loading,
  message,
  notificationAccountFilter,
  notificationFilter,
  notifications,
  overview,
  page,
  pendingRepairAccount,
  pendingSwitchAccount,
  realAccounts,
  recommendationList,
  recommendedSwitchAccount,
  regressionChecks,
  releaseDiagnostic,
  selectedAccount,
  selectedAccountDetail,
  selectedAccountSessions,
  selectedAccountNotifications,
  settings,
  startupHealth,
  startupSummary,
  submitting,
  switchLogs,
  usageDisplay,
  accountSwitchabilitySummary,
  onBeginEditKeyProfile,
  onBeginRepairFlow,
  onCancelEditKeyProfile,
  onCleanupDebugData,
  onCloseSelectedAccount,
  onCloseSwitchConfirm,
  onCopyAccountDiagnostic,
  onCreateKeyProfile,
  onDiagnoseBindEnvironment,
  onExecuteSwitchAccount,
  onImportCodexLocalSessions,
  onGoAccounts,
  onGoHandoff,
  onGoStability,
  onHandleBindCurrentAccount,
  onLoadAccountDetail,
  onLoadBootstrap,
  onLoadReleaseDiagnostic,
  onLoadStartupHealth,
  onMakeDefault,
  onNavigate,
  onOpenAccountDetail,
  onRefreshOverview,
  onRemoveAccount,
  onRepairAuth,
  onSampleNow,
  onSaveSettings,
  onActivateIdentity,
  onSetBindingNickname,
  onSetKeyProfileDraft,
  onSetNotificationAccountFilter,
  onSetNotificationFilter,
  onSetSettings,
  onShowSwitchConfirm,
  onStartLoginFlow,
  onVerifyAccount,
  relatedAccountForNotification,
}: AppContentProps) {
  const startupTone = startupSummary?.items.some((item) => item.tone === "error")
    ? "error"
    : startupSummary?.items.some((item) => item.tone === "warning")
      ? "warning"
      : "healthy";
  const startupLeadItem = startupSummary?.items[0] ?? null;

  const sidebarIdentity = (
    <div className="context-sidebar__identity-copy">
      <p className="eyebrow">账号调度与项目会话工作台</p>
      <h3>{pageTitles[page]}</h3>
      <p>{loading ? "正在读取本地状态" : "把当前账号、推荐切换和关键操作集中在一侧。"}</p>
      {!loading && overview ? (
        <p className="context-sidebar__identity-status">
          当前身份：{identitySummaryText(activeIdentity)}
        </p>
      ) : null}
    </div>
  );

  const shellActions = {
    refresh: {
      key: "refresh",
      label: "刷新状态",
      tone: "primary" as const,
      disabled: loading || submitting,
      onClick: () => void onLoadBootstrap(),
    },
    sample: {
      key: "sample",
      label: "立即采样",
      tone: "secondary" as const,
      disabled: loading || submitting || activeIdentity?.kind === "third_party_key",
      onClick: () => void onSampleNow(),
    },
    dashboard: {
      key: "dashboard",
      label: "返回仪表盘",
      tone: "ghost" as const,
      disabled: page === "dashboard",
      onClick: () => onNavigate("dashboard"),
    },
  };

  const sidebarActions: SidebarAction[] = [
    shellActions.refresh,
    shellActions.sample,
    ...(page === "dashboard" ? [] : [shellActions.dashboard]),
  ];

  const sidebarSections: SidebarSectionData[] = [
    {
      key: "runtime",
      title: "运行状态",
      items: [
        { label: "当前页面", value: pageTitles[page] },
        { label: "当前身份", value: identitySummaryText(activeIdentity), tone: activeIdentity ? "success" : "muted" },
        { label: "加载状态", value: loading ? "加载中" : "就绪", tone: loading ? "warning" : "success" },
        { label: "提交状态", value: submitting ? "进行中" : "空闲", tone: submitting ? "warning" : "muted" },
      ],
    },
    {
      key: "accounts",
      title: "账号概览",
      items: [
        { label: "活跃账号", value: activeAccount?.nickname ?? "未设置", tone: activeAccount ? "success" : "muted" },
        { label: "可切换身份", value: `${identityAssets.length} 个`, tone: identityAssets.length > 0 ? "default" : "muted" },
        {
          label: "官方登录",
          value: currentLogin?.logged_in ? (currentLogin.email ?? currentLogin.account_id ?? "已登录") : "未检测到",
          tone: currentLogin?.logged_in ? "success" : "warning",
        },
        { label: "真实账号", value: `${realAccounts.length} 个`, tone: realAccounts.length > 0 ? "default" : "muted" },
      ],
    },
    {
      key: "focus",
      title: "当前聚焦",
      items: [
        {
          label: "切换建议",
          value: recommendedIdentity?.title ?? recommendedSwitchAccount?.nickname ?? "暂无",
          tone: recommendedIdentity ? "warning" : "muted",
        },
      ],
    },
  ];

  const toolbarNode = (
    <div className="toolbar-row">
      <button className="btn btn-secondary" onClick={shellActions.sample.onClick} type="button" disabled={shellActions.sample.disabled}>
        {shellActions.sample.label}
      </button>
      <button className="btn btn-primary" onClick={shellActions.refresh.onClick} type="button" disabled={shellActions.refresh.disabled}>
        {shellActions.refresh.label}
      </button>
    </div>
  );

  const pageBody = (
    <AppPageBody
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
      currentLoginIsBound={currentLoginIsBound}
      editingKeyProfileId={editingKeyProfileId}
      localProjects={localProjects}
      sessionRecords={sessionRecords}
      codexLocalSessionCandidates={codexLocalSessionCandidates}
      importingCodexSessions={importingCodexSessions}
      activeIdentity={activeIdentity}
      dashboardIdentityAssets={dashboardIdentityAssets}
      identityAssets={identityAssets}
      recommendedIdentity={recommendedIdentity}
      keyProfileDraft={keyProfileDraft}
      keyProfileAction={keyProfileAction}
      keyProfileFormFeedback={keyProfileFormFeedback}
      hasAuthIssue={hasAuthIssue}
      lastOperationError={lastOperationError}
      message={message}
      notificationAccountFilter={notificationAccountFilter}
      notificationFilter={notificationFilter}
      notifications={notifications}
      overview={overview!}
      page={page}
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
      onBeginEditKeyProfile={onBeginEditKeyProfile}
      onBeginRepairFlow={onBeginRepairFlow}
      onCancelEditKeyProfile={onCancelEditKeyProfile}
      onCleanupDebugData={onCleanupDebugData}
      onCloseSelectedAccount={onCloseSelectedAccount}
      onCloseSwitchConfirm={onCloseSwitchConfirm}
      onCopyAccountDiagnostic={onCopyAccountDiagnostic}
      onCreateKeyProfile={onCreateKeyProfile}
      onDiagnoseBindEnvironment={onDiagnoseBindEnvironment}
      onExecuteSwitchAccount={onExecuteSwitchAccount}
      onImportCodexLocalSessions={onImportCodexLocalSessions}
      onGoAccounts={onGoAccounts}
      onGoHandoff={onGoHandoff}
      onGoStability={onGoStability}
      onHandleBindCurrentAccount={onHandleBindCurrentAccount}
      onLoadAccountDetail={onLoadAccountDetail}
      onLoadBootstrap={onLoadBootstrap}
      onLoadReleaseDiagnostic={onLoadReleaseDiagnostic}
      onLoadStartupHealth={onLoadStartupHealth}
      onMakeDefault={onMakeDefault}
      onOpenAccountDetail={onOpenAccountDetail}
      onRefreshOverview={onRefreshOverview}
      onRemoveAccount={onRemoveAccount}
      onRepairAuth={onRepairAuth}
      onSampleNow={onSampleNow}
      onSaveSettings={onSaveSettings}
      onActivateIdentity={onActivateIdentity}
      onSetBindingNickname={onSetBindingNickname}
      onSetKeyProfileDraft={onSetKeyProfileDraft}
      onSetNotificationAccountFilter={onSetNotificationAccountFilter}
      onSetNotificationFilter={onSetNotificationFilter}
      onSetSettings={onSetSettings}
      onShowSwitchConfirm={onShowSwitchConfirm}
      onStartLoginFlow={onStartLoginFlow}
      onVerifyAccount={onVerifyAccount}
      relatedAccountForNotification={relatedAccountForNotification}
    />
  );

  const shellPlaceholder = (
    <section className="workspace-card shell-placeholder">
      <div className="shell-placeholder__copy">
        <p className="eyebrow">{loading ? "正在准备工作台" : "工作台暂时不可用"}</p>
        <h2>{loading ? "正在加载账号与调度信息" : "暂时还无法显示完整页面"}</h2>
        <p>
          {loading
            ? "正在读取本地账号、采样和通知数据，界面会在初始化完成后自动刷新。"
            : message || "当前没有拿到工作台概览数据，请先刷新状态或检查本地运行环境。"}
        </p>
      </div>
      <div className="shell-placeholder__actions">
        <button className="btn btn-primary" type="button" disabled={loading || submitting} onClick={() => void onLoadBootstrap()}>
          重新加载
        </button>
        <button className="btn btn-secondary" type="button" disabled={loading || submitting} onClick={() => void onSampleNow()}>
          立即采样
        </button>
      </div>
    </section>
  );

  return (
    <div className="app-shell">
      <main className="main-panel">
        <AppFrame
          brandSubtitle={currentIdentitySubtitle}
          currentPage={page}
          pageTitle={pageTitles[page]}
          onNavigate={onNavigate}
          sidebar={
            <ContextSidebar
              identity={sidebarIdentity}
              actions={sidebarActions}
              sections={sidebarSections}
              footer={
                <div className="context-sidebar__footer-stack">
                  <p className="context-sidebar__footer-message">{message || "等待下一次操作"}</p>
                  {startupSummary ? (
                    <section className="context-sidebar__status-card">
                      <div className="context-sidebar__status-card-head">
                        <span className="eyebrow">系统状态</span>
                        <span className={`status-tag ${startupTone}`}>{startupTone === "healthy" ? "正常" : startupTone === "warning" ? "注意" : "异常"}</span>
                      </div>
                      <strong>{startupSummary.title}</strong>
                      <p>{startupLeadItem ? `${startupLeadItem.label} · ${startupLeadItem.detail}` : startupSummary.summary}</p>
                      <div className="context-sidebar__status-card-actions">
                        <button className="btn btn-secondary" type="button" onClick={onGoStability}>
                          看诊断
                        </button>
                        {startupSummary.items.some((item) => item.action === "accounts") ? (
                          <button className="btn btn-ghost" type="button" onClick={onGoAccounts}>
                            账号中心
                          </button>
                        ) : null}
                      </div>
                    </section>
                  ) : null}
                </div>
              }
            />
          }
          toolbar={toolbarNode}
        >
          {overview ? pageBody : shellPlaceholder}
        </AppFrame>
      </main>
    </div>
  );
}
