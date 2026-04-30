import { AccountsPage } from "./AccountsPage";
import { AccountDetailDrawer } from "./AccountDetailDrawer";
import { DashboardPage } from "./DashboardPage";
import { HandoffPage } from "./HandoffPage";
import { NotificationsPage } from "./NotificationsPage";
import { SettingsPage } from "./SettingsPage";
import { StabilityPage } from "./StabilityPage";
import { SwitchConfirmPanel } from "./SwitchConfirmPanel";
import type { PageKey } from "../shell/layout";
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

type NotificationFilter = "all" | NotificationSourceType;

type AppBannerStackProps = {
  activeAccount: Account | null;
  currentLogin: {
    logged_in: boolean;
    email: string | null;
    account_id: string | null;
    is_bound: boolean;
  } | null;
  currentLoginIsBound: boolean;
  message: string;
  onHandleBindCurrentAccount: () => void | Promise<void>;
  onShowSwitchConfirm: (id: number) => void | Promise<void>;
  page: PageKey;
  recommendedSwitchAccount: Account | null;
  settings: AppSettings;
  submitting: boolean;
};

export type AppPageBodyProps = {
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
  dashboardIdentityAssets: IdentityAsset[];
  identityAssets: IdentityAsset[];
  recommendedIdentity: IdentityAsset | null;
  keyProfileDraft: CreateKeyProfileInput;
  keyProfileAction: { profileId: number | null; kind: "save" | "update" | "activate" } | null;
  keyProfileFormFeedback: string;
  hasAuthIssue: (account: Account) => boolean;
  lastOperationError: string | null;
  message: string;
  notificationAccountFilter: number | "all";
  notificationFilter: NotificationFilter;
  notifications: NotificationItem[];
  overview: DashboardOverview;
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

function AppBannerStack({
  activeAccount,
  currentLogin,
  currentLoginIsBound,
  message,
  onHandleBindCurrentAccount,
  onShowSwitchConfirm,
  page,
  recommendedSwitchAccount,
  settings,
  submitting,
}: AppBannerStackProps) {
  if (page === "dashboard") {
    return null;
  }

  return (
    <>
      {settings.prefer_official_upgrade ? (
        <div className="banner">当前已开启官方扩容优先策略：优先官方扩容，账号切换仅作为备选动作。</div>
      ) : null}

      {currentLogin?.logged_in && !currentLoginIsBound ? (
        <div className="banner">
          <span>
            检测到当前 Codex 官方登录态：{currentLogin.email ?? currentLogin.account_id ?? "未知账号"}，但还没有加入账号列表。
          </span>
          <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onHandleBindCurrentAccount()}>
            绑定当前登录账号
          </button>
        </div>
      ) : null}

      {message ? <div className="banner">{message}</div> : null}

      {activeAccount?.status === "warning" && recommendedSwitchAccount ? (
        <div className="banner action-banner warning-banner">
          <span>
            当前账号处于预警区间，推荐切换到 {recommendedSwitchAccount.nickname}。
            原因：目标账号未满额且登录态有效，切换后会自动采样。
          </span>
          <button className="btn btn-secondary" type="button" onClick={() => void onShowSwitchConfirm(recommendedSwitchAccount.id)}>
            查看切换确认
          </button>
        </div>
      ) : null}
    </>
  );
}

export function AppPageBody({
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
  dashboardIdentityAssets,
  identityAssets,
  recommendedIdentity,
  keyProfileDraft,
  keyProfileAction,
  keyProfileFormFeedback,
  hasAuthIssue,
  lastOperationError,
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
  onHandleBindCurrentAccount,
  onLoadAccountDetail,
  onLoadBootstrap,
  onLoadReleaseDiagnostic,
  onLoadStartupHealth,
  onMakeDefault,
  onOpenAccountDetail,
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
}: AppPageBodyProps) {
  return (
    <>
      <AppBannerStack
        activeAccount={activeAccount}
        currentLogin={currentLogin}
        currentLoginIsBound={currentLoginIsBound}
        message={message}
        onHandleBindCurrentAccount={onHandleBindCurrentAccount}
        onShowSwitchConfirm={onShowSwitchConfirm}
        page={page}
        recommendedSwitchAccount={recommendedSwitchAccount}
        settings={settings}
        submitting={submitting}
      />

      {page === "dashboard" ? (
        <DashboardPage
          activeAccount={activeAccount}
          activeIdentity={activeIdentity}
          autoSampleStatus={autoSampleStatus}
          canSwitch={canSwitch}
          dashboardIdentityAssets={dashboardIdentityAssets}
          onGoAccounts={onGoAccounts}
          onGoHandoff={onGoHandoff}
          onActivateIdentity={onActivateIdentity}
          onOpenAccountDetail={onOpenAccountDetail}
          onRefresh={onLoadBootstrap}
          onSampleNow={onSampleNow}
          onShowSwitchConfirm={onShowSwitchConfirm}
          overview={overview}
          recommendationList={recommendationList}
          recommendedIdentity={recommendedIdentity}
          recommendedSwitchAccount={recommendedSwitchAccount}
          submitting={submitting}
          usageDisplay={usageDisplay}
        />
      ) : null}

      {page === "accounts" ? (
        <AccountsPage
          activeAccount={activeAccount}
          activeIdentity={activeIdentity}
          bindDiagnostic={bindDiagnostic}
          bindingNickname={bindingNickname}
          canSwitch={canSwitch}
          currentLogin={currentLogin}
          credentialProfiles={credentialProfiles}
          editingKeyProfileId={editingKeyProfileId}
          handleBindCurrentAccount={onHandleBindCurrentAccount}
          identityAssets={identityAssets}
          keyProfileAction={keyProfileAction}
          keyProfileFormFeedback={keyProfileFormFeedback}
          keyProfileDraft={keyProfileDraft}
          lastOperationError={lastOperationError}
          onActivateIdentity={onActivateIdentity}
          onBeginEditKeyProfile={onBeginEditKeyProfile}
          onBeginRepairFlow={onBeginRepairFlow}
          onCancelEditKeyProfile={onCancelEditKeyProfile}
          onCreateKeyProfile={onCreateKeyProfile}
          onDiagnoseBindEnvironment={onDiagnoseBindEnvironment}
          onMakeDefault={onMakeDefault}
          onOpenAccountDetail={onOpenAccountDetail}
          onRefresh={onLoadBootstrap}
          onRemoveAccount={onRemoveAccount}
          onRepairAuth={onRepairAuth}
          onSetBindingNickname={onSetBindingNickname}
          onSetKeyProfileDraft={onSetKeyProfileDraft}
          onStartLoginFlow={onStartLoginFlow}
          onSwitchAccount={onShowSwitchConfirm}
          onVerifyAccount={onVerifyAccount}
          pendingRepairAccount={pendingRepairAccount}
          realAccounts={realAccounts}
          submitting={submitting}
        />
      ) : null}

      {page === "handoff" ? (
        <HandoffPage
          identityAssets={identityAssets}
          activeIdentity={activeIdentity}
          localProjects={localProjects}
          sessionRecords={sessionRecords}
          codexLocalSessionCandidates={codexLocalSessionCandidates}
          importingCodexSessions={importingCodexSessions}
          onImportCodexLocalSessions={onImportCodexLocalSessions}
        />
      ) : null}

      {page === "notifications" ? (
        <NotificationsPage
          notificationAccountFilter={notificationAccountFilter}
          notificationFilter={notificationFilter}
          notifications={notifications}
          onOpenAccountDetail={onOpenAccountDetail}
          onSetNotificationAccountFilter={onSetNotificationAccountFilter}
          onSetNotificationFilter={onSetNotificationFilter}
          realAccounts={realAccounts}
          relatedAccountForNotification={relatedAccountForNotification}
          settingsPreferOfficialUpgrade={settings.prefer_official_upgrade}
        />
      ) : null}

      {page === "stability" ? (
        <StabilityPage
          cleanupPreview={cleanupPreview}
          notifications={notifications}
          onCleanupDebugData={onCleanupDebugData}
          onLoadReleaseDiagnostic={onLoadReleaseDiagnostic}
          onLoadStartupHealth={onLoadStartupHealth}
          regressionChecks={regressionChecks}
          releaseDiagnostic={releaseDiagnostic}
          relatedAccountForNotification={relatedAccountForNotification}
          startupHealth={startupHealth}
          submitting={submitting}
        />
      ) : null}

      {page === "settings" ? (
        <SettingsPage
          autoSampleStatus={autoSampleStatus}
          continueSamplingWhenHidden={continueSamplingWhenHidden}
          onSaveSettings={onSaveSettings}
          onUpdateSettings={onSetSettings}
          settings={settings}
        />
      ) : null}

      {selectedAccount ? (
        <AccountDetailDrawer
          account={selectedAccount}
          accountDetail={selectedAccountDetail}
          accountSwitchabilitySummary={accountSwitchabilitySummary}
          authStateText={authStateText}
          canSwitch={canSwitch}
          hasAuthIssue={hasAuthIssue}
          notifications={selectedAccountNotifications}
          onBeginRepairFlow={onBeginRepairFlow}
          onClose={onCloseSelectedAccount}
          onCopyDiagnostic={onCopyAccountDiagnostic}
          onLoadDetail={onLoadAccountDetail}
          onOpenSessionsPage={onGoHandoff}
          sessions={selectedAccountSessions}
          onSampleNow={onSampleNow}
          onSwitchAccount={onShowSwitchConfirm}
          submitting={submitting}
          switchLogs={switchLogs}
        />
      ) : null}

      {pendingSwitchAccount ? (
        <SwitchConfirmPanel
          activeAccount={activeAccount}
          accountSwitchabilitySummary={accountSwitchabilitySummary}
          canSwitch={canSwitch}
          onClose={onCloseSwitchConfirm}
          onConfirm={onExecuteSwitchAccount}
          pendingSwitchAccount={pendingSwitchAccount}
          submitting={submitting}
        />
      ) : null}
    </>
  );
}
