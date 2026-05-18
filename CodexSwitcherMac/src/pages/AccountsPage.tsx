import { IdentityAssetTable } from "../components/IdentityAssetTable";
import { MetricTile } from "../components/MetricTile";
import { SectionHeader } from "../components/SectionHeader";
import type {
  Account,
  BindEnvironmentDiagnostic,
  CreateKeyProfileInput,
  CredentialProfile,
  CurrentCodexLogin,
  ThirdPartyKeyUsageSummary,
} from "../types";
import type { IdentityAsset } from "./identityViewModel";
import { identitySummaryText } from "./identityViewModel";
import {
  buildLoginGuideSteps,
  currentLoginIsBound,
  currentLoginLabel,
  currentLoginMatchesAccount,
  diagnosticAdvice,
} from "./viewModel";

type AccountsPageProps = {
  activeAccount: Account | null;
  activeIdentity: IdentityAsset | null;
  bindDiagnostic: BindEnvironmentDiagnostic | null;
  bindingNickname: string;
  canSwitch: (account: Account) => boolean;
  credentialProfiles: CredentialProfile[];
  currentLogin: CurrentCodexLogin | null;
  editingKeyProfileId: number | null;
  handleBindCurrentAccount: () => void | Promise<void>;
  identityAssets: IdentityAsset[];
  keyProfileDraft: CreateKeyProfileInput;
  keyProfileAction: { profileId: number | null; kind: "save" | "update" | "activate" | "delete" } | null;
  keyProfileFormFeedback: string;
  lastOperationError: string | null;
  onActivateIdentity: (asset: IdentityAsset) => void | Promise<void>;
  onBeginEditKeyProfile: (profile: CredentialProfile) => void;
  onBeginRepairFlow: (id: number) => void | Promise<void>;
  onCancelEditKeyProfile: () => void;
  onCreateKeyProfile: () => void | Promise<void>;
  onDeleteKeyProfile: (profile: CredentialProfile) => void | Promise<void>;
  onDiagnoseBindEnvironment: () => void | Promise<void>;
  onMakeDefault: (id: number) => void | Promise<void>;
  onOpenAccountDetail: (account: Account) => void;
  onRefresh: () => void | Promise<void>;
  onRemoveAccount: (id: number) => void | Promise<void>;
  onRepairAuth: (id: number) => void | Promise<void>;
  onSetBindingNickname: (value: string) => void;
  onSetKeyProfileDraft: (value: CreateKeyProfileInput) => void;
  onStartLoginFlow: () => void | Promise<void>;
  onSwitchAccount: (id: number) => void | Promise<void>;
  onVerifyAccount: (id: number) => void | Promise<void>;
  pendingDeleteKeyProfileId: number | null;
  pendingRepairAccount: Account | null;
  realAccounts: Account[];
  submitting: boolean;
};

function AccountsEntryTools({
  activeAccount,
  bindDiagnostic,
  bindingNickname,
  currentLogin,
  handleBindCurrentAccount,
  lastOperationError,
  onBeginRepairFlow,
  onDiagnoseBindEnvironment,
  onRefresh,
  onSetBindingNickname,
  onStartLoginFlow,
  pendingRepairAccount,
  realAccounts,
  submitting,
}: {
  activeAccount: Account | null;
  bindDiagnostic: BindEnvironmentDiagnostic | null;
  bindingNickname: string;
  currentLogin: CurrentCodexLogin | null;
  handleBindCurrentAccount: () => void | Promise<void>;
  lastOperationError: string | null;
  onBeginRepairFlow: (id: number) => void | Promise<void>;
  onDiagnoseBindEnvironment: () => void | Promise<void>;
  onRefresh: () => void | Promise<void>;
  onSetBindingNickname: (value: string) => void;
  onStartLoginFlow: () => void | Promise<void>;
  pendingRepairAccount: Account | null;
  realAccounts: Account[];
  submitting: boolean;
}) {
  const loginGuideSteps = buildLoginGuideSteps({
    activeAccount,
    bindDiagnostic,
    currentLogin,
    currentLoginIsBound: currentLoginIsBound(currentLogin, realAccounts),
    realAccounts,
  });

  return (
    <div className="accounts-tools-grid">
      <article className="workspace-card accounts-tool-card">
        <SectionHeader eyebrow="绑定与登录" title="官方登录入口" description="先登录，再绑定当前账号，再校验和切换。" />
        <div className="login-guide-grid">
          {loginGuideSteps.map((step) => (
            <div className={`login-guide-step ${step.done ? "done" : ""}`} key={step.title}>
              <span>{step.done ? "已完成" : "待处理"}</span>
              <strong>{step.title}</strong>
              <p>{step.detail}</p>
            </div>
          ))}
        </div>
        <div className="form-grid accounts-form-grid">
          <label>
            账号昵称
            <input
              value={bindingNickname}
              placeholder={currentLogin?.email ?? currentLogin?.account_id ?? "完成官方登录后自动填入"}
              onChange={(event) => onSetBindingNickname(event.currentTarget.value)}
            />
          </label>
          <div className="row-actions compact accounts-action-row">
            <button className="btn btn-primary" type="button" disabled={submitting} onClick={() => void onStartLoginFlow()}>
              {submitting ? "处理中..." : "开始官方登录"}
            </button>
            <button
              className="btn btn-secondary"
              type="button"
              disabled={submitting || !bindingNickname.trim()}
              onClick={() => void handleBindCurrentAccount()}
            >
              {submitting ? "绑定中..." : "绑定当前已登录账号"}
            </button>
            <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onDiagnoseBindEnvironment()}>
              诊断绑定环境
            </button>
          </div>
        </div>
      </article>

      <article className="workspace-card accounts-tool-card">
        <SectionHeader eyebrow="当前状态" title="登录与诊断摘要" description="这里集中放绑定状态、修复提示和环境诊断。" />
        <div className="binding-note accounts-note-card">
          <strong>当前官方登录态</strong>
          <p>登录账号：{currentLogin?.email ?? "未读取到邮箱"}</p>
          <p>官方账号 ID：{currentLogin?.account_id ?? "--"}</p>
          <p>绑定状态：{currentLoginIsBound(currentLogin, realAccounts) ? "已在账号列表中" : "未绑定到账号列表"}</p>
        </div>
        {pendingRepairAccount ? (
          <div className="binding-note accounts-note-card accounts-warning-card">
            <strong>一键重绑进行中</strong>
            <p>目标账号：{pendingRepairAccount.nickname}</p>
            <p>当前登录：{currentLoginLabel(currentLogin)}</p>
            <div className="row-actions compact accounts-action-row">
              <button className="btn btn-secondary" type="button" onClick={() => void onBeginRepairFlow(pendingRepairAccount.id)}>
                重新打开登录流程
              </button>
              <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onRefresh()}>
                我已登录，刷新检测
              </button>
            </div>
          </div>
        ) : null}
        <div className="binding-note accounts-note-card">
          <strong>采样失败诊断卡</strong>
          <p>最近错误：{lastOperationError ?? "暂无"}</p>
          <p>建议动作：{diagnosticAdvice(lastOperationError)}</p>
        </div>
        {bindDiagnostic ? (
          <div className="binding-note accounts-note-card">
            <strong>环境诊断结果</strong>
            <p>Codex 配置目录：{bindDiagnostic.codex_config_dir}</p>
            <p>CLI：{bindDiagnostic.cli_available ?? "未找到"}</p>
            <p>凭证文件：{bindDiagnostic.auth_exists ? "存在" : "不存在"}</p>
          </div>
        ) : null}
      </article>
    </div>
  );
}

function maskedSecretLooksLikeUrl(profile: CredentialProfile) {
  return profile.masked_secret?.toLowerCase().startsWith("http") ?? false;
}

function formatUsageMoney(value: number | null | undefined, unit: string | null | undefined) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  if ((unit?.trim() || "") === "额度") return `${Math.round(value)} 额度`;
  return `${unit?.trim() || "USD"} ${value.toFixed(2)}`;
}

function keyUsageLines(summary: ThirdPartyKeyUsageSummary | null | undefined) {
  if (!summary || summary.status !== "ready") {
    return [];
  }

  const lines: string[] = [];
  if (summary.balance !== null || summary.remaining !== null) {
    lines.push(`余额 ${formatUsageMoney(summary.remaining ?? summary.balance, summary.unit)}`);
  }
  lines.push(...summary.detail_items.map((item) => `${item.label} ${item.value}`));
  if (summary.usage_provider_type === "new_api" && summary.plan_name) {
    lines.push(summary.plan_name);
  }
  return lines;
}

function KeyProfileAssets({
  editingKeyProfileId,
  keyProfileAction,
  keyProfileFormFeedback,
  keyProfileDraft,
  onCancelEditKeyProfile,
  onCreateKeyProfile,
  onSetKeyProfileDraft,
  submitting,
}: {
  editingKeyProfileId: number | null;
  keyProfileAction: { profileId: number | null; kind: "save" | "update" | "activate" | "delete" } | null;
  keyProfileFormFeedback: string;
  keyProfileDraft: CreateKeyProfileInput;
  onCancelEditKeyProfile: () => void;
  onCreateKeyProfile: () => void | Promise<void>;
  onSetKeyProfileDraft: (value: CreateKeyProfileInput) => void;
  submitting: boolean;
}) {
  const formActionActive = keyProfileAction?.kind === "save" || keyProfileAction?.kind === "update";
  const savingLabel = editingKeyProfileId ? "更新 Key" : "保存 Key";
  return (
    <article className="workspace-card accounts-workspace__list key-profile-assets">
      <SectionHeader
        eyebrow="第三方 Key"
        title="Key 账号资产"
        description="保存 key 后可启用为当前 Codex 运行身份。"
      />

      <div className="form-grid accounts-form-grid key-profile-form">
        <label>
          供应商
          <input
            value={keyProfileDraft.provider}
            onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, provider: event.currentTarget.value })}
          />
        </label>
        <label>
          昵称
          <input
            value={keyProfileDraft.nickname}
            placeholder="例如 YuChat 备用 Key"
            onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, nickname: event.currentTarget.value })}
          />
        </label>
        <label>
          Base URL
          <input
            value={keyProfileDraft.base_url}
            placeholder="https://sub2api.yuchat.top"
            onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, base_url: event.currentTarget.value })}
          />
        </label>
        <label>
          模型
          <input
            value={keyProfileDraft.model}
            onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, model: event.currentTarget.value })}
          />
        </label>
        <label>
          API Key
          <input
            type="password"
            autoComplete="new-password"
            value={keyProfileDraft.api_key}
            placeholder={editingKeyProfileId ? "留空不改；要修复请填真实 API Key，不是 Base URL" : "真实 API Key，不是 Base URL"}
            onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, api_key: event.currentTarget.value })}
          />
        </label>
        <label>
          余额统计类型
          <select
            value={keyProfileDraft.usage_provider_type}
            onChange={(event) => onSetKeyProfileDraft({
              ...keyProfileDraft,
              usage_provider_type: event.currentTarget.value as "none" | "sub2api" | "new_api",
              usage_query_user: event.currentTarget.value === "new_api" ? keyProfileDraft.usage_query_user : "",
              usage_query_app_version: event.currentTarget.value === "new_api"
                ? (keyProfileDraft.usage_query_app_version || "3.1.0")
                : "",
              usage_access_token: event.currentTarget.value === "new_api" ? keyProfileDraft.usage_access_token : "",
            })}
          >
            <option value="none">不显示余额统计</option>
            <option value="sub2api">sub2api / 语聊</option>
            <option value="new_api">newApi / oneTop</option>
          </select>
        </label>
        {keyProfileDraft.usage_provider_type === "new_api" ? (
          <>
            <p className="helper-text">newApi / oneTop 的余额统计走访问令牌，不复用上面的 API Key。</p>
            <label>
              访问令牌
              <input
                type="password"
                autoComplete="new-password"
                value={keyProfileDraft.usage_access_token}
                placeholder={editingKeyProfileId ? "留空不改；填 oneTop 个人安全设置里的访问令牌" : "oneTop 个人安全设置里的访问令牌"}
                onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, usage_access_token: event.currentTarget.value })}
              />
            </label>
            <label>
              New-Api-User
              <input
                value={keyProfileDraft.usage_query_user}
                placeholder="例如 123，可选"
                onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, usage_query_user: event.currentTarget.value })}
              />
            </label>
            <label>
              App-Version
              <input
                value={keyProfileDraft.usage_query_app_version}
                placeholder="默认 3.1.0，可选"
                onChange={(event) => onSetKeyProfileDraft({ ...keyProfileDraft, usage_query_app_version: event.currentTarget.value })}
              />
            </label>
          </>
        ) : null}
        <div className="row-actions compact accounts-action-row">
          <button className="btn btn-primary" type="button" disabled={submitting} onClick={() => void onCreateKeyProfile()}>
            {formActionActive ? (editingKeyProfileId ? "更新中..." : "保存中...") : savingLabel}
          </button>
          {editingKeyProfileId ? (
            <button className="btn btn-secondary" type="button" disabled={submitting} onClick={onCancelEditKeyProfile}>
              取消编辑
            </button>
          ) : null}
        </div>
        {keyProfileFormFeedback ? (
          <p className={`key-profile-feedback ${keyProfileFormFeedback.includes("失败") || keyProfileFormFeedback.includes("不能") || keyProfileFormFeedback.includes("请输入") ? "error" : ""}`}>
            {keyProfileFormFeedback}
          </p>
        ) : null}
      </div>
    </article>
  );
}

export function AccountsPage({
  activeAccount,
  activeIdentity,
  bindDiagnostic,
  bindingNickname,
  canSwitch,
  credentialProfiles,
  currentLogin,
  editingKeyProfileId,
  handleBindCurrentAccount,
  identityAssets,
  keyProfileDraft,
  keyProfileAction,
  keyProfileFormFeedback,
  lastOperationError,
  onActivateIdentity,
  onBeginEditKeyProfile,
  onBeginRepairFlow,
  onCancelEditKeyProfile,
  onCreateKeyProfile,
  onDeleteKeyProfile,
  onDiagnoseBindEnvironment,
  onMakeDefault,
  onOpenAccountDetail,
  onRefresh,
  onRemoveAccount,
  onRepairAuth,
  onSetBindingNickname,
  onSetKeyProfileDraft,
  onStartLoginFlow,
  onSwitchAccount,
  onVerifyAccount,
  pendingDeleteKeyProfileId,
  pendingRepairAccount,
  realAccounts,
  submitting,
}: AccountsPageProps) {
  const switchableAccounts = realAccounts.filter((account) => canSwitch(account));
  const repairAccounts = realAccounts.filter((account) => !canSwitch(account));
  const keyProfiles = credentialProfiles.filter((profile) => profile.profile_kind === "third_party_key");
  const switchableKeys = keyProfiles.filter((profile) => !profile.is_active);

  return (
    <section className="accounts-workspace">
      <div className="accounts-workspace__summary">
        <MetricTile label="真实账号" value={`${realAccounts.length} 个`} />
        <MetricTile label="可切换身份" value={`${switchableAccounts.length + switchableKeys.length} 个`} tone="success" />
        <MetricTile label="需关注" value={`${repairAccounts.length} 个`} tone="warning" />
        <MetricTile label="当前身份" value={identitySummaryText(activeIdentity)} />
      </div>

      <AccountsEntryTools
        activeAccount={activeAccount}
        bindDiagnostic={bindDiagnostic}
        bindingNickname={bindingNickname}
        currentLogin={currentLogin}
        handleBindCurrentAccount={handleBindCurrentAccount}
        lastOperationError={lastOperationError}
        onBeginRepairFlow={onBeginRepairFlow}
        onDiagnoseBindEnvironment={onDiagnoseBindEnvironment}
        onRefresh={onRefresh}
        onSetBindingNickname={onSetBindingNickname}
        onStartLoginFlow={onStartLoginFlow}
        pendingRepairAccount={pendingRepairAccount}
        realAccounts={realAccounts}
        submitting={submitting}
      />

      <div className="accounts-workspace__body">
        <article className="workspace-card accounts-workspace__list">
          <SectionHeader
            eyebrow="账号资产"
            title="统一身份资产列表"
            description="官方账号和第三方 Key 共用同一个切换入口；Key 只保留启用和编辑。"
            actions={
              <button className="btn btn-secondary" type="button" onClick={() => void onRefresh()}>
                刷新状态
              </button>
            }
          />
          <IdentityAssetTable
            assets={identityAssets}
            canSwitch={canSwitch}
            onSelectAccount={onOpenAccountDetail}
            renderActions={(asset) => asset.kind === "third_party_key" ? (
              <div className="account-list-table__tool-actions">
                <div className="account-list-table__tool-row">
                  <button
                    className="btn btn-primary"
                    type="button"
                    disabled={submitting || asset.isActive}
                    onClick={() => void onActivateIdentity(asset)}
                  >
                    {keyProfileAction?.profileId === asset.profile.id && keyProfileAction.kind === "activate" ? "启用中..." : asset.isActive ? "当前 Key" : "启用"}
                  </button>
                  <button
                    className="btn btn-secondary"
                    type="button"
                    disabled={submitting}
                    onClick={() => onBeginEditKeyProfile(asset.profile)}
                  >
                    编辑
                  </button>
                  <button
                    className="btn btn-danger"
                    type="button"
                    disabled={submitting || asset.isActive}
                    onClick={() => void onDeleteKeyProfile(asset.profile)}
                  >
                    {keyProfileAction?.profileId === asset.profile.id && keyProfileAction.kind === "delete"
                      ? "删除中..."
                      : pendingDeleteKeyProfileId === asset.profile.id ? "确认删除" : "删除"}
                  </button>
                </div>
                {maskedSecretLooksLikeUrl(asset.profile) ? (
                  <p className="key-profile-feedback error">
                    API Key 像 Base URL，请编辑修复。
                  </p>
                ) : null}
                {keyUsageLines(asset.profile.usage_summary).length ? (
                  <div className="key-usage-summary">
                    {keyUsageLines(asset.profile.usage_summary).map((line) => (
                      <p key={`${asset.profile.id}-${line}`}>{line}</p>
                    ))}
                  </div>
                ) : null}
              </div>
            ) : asset.account ? (
              <div className="account-list-table__tool-actions">
                <div className="account-list-table__tool-row">
                  <button className="btn btn-secondary" type="button" onClick={() => onOpenAccountDetail(asset.account!)}>
                    详情
                  </button>
                  <button className="btn btn-secondary" type="button" onClick={() => void onMakeDefault(asset.account!.id)}>
                    默认
                  </button>
                  {canSwitch(asset.account) ? (
                    <button className="btn btn-primary" type="button" disabled={submitting} onClick={() => void onSwitchAccount(asset.account!.id)}>
                      切换
                    </button>
                  ) : null}
                </div>
                <div className="account-list-table__tool-row">
                  <button
                    className="btn btn-ghost"
                    type="button"
                    onClick={() => void onVerifyAccount(asset.account!.id)}
                    disabled={submitting || !currentLoginMatchesAccount(currentLogin, asset.account)}
                  >
                    {currentLoginMatchesAccount(currentLogin, asset.account) ? "校验" : "先登录"}
                  </button>
                  {!canSwitch(asset.account) ? (
                    <>
                      <button className="btn btn-ghost" type="button" onClick={() => void onBeginRepairFlow(asset.account!.id)}>
                        重绑
                      </button>
                      <button className="btn btn-ghost" type="button" onClick={() => void onRepairAuth(asset.account!.id)}>
                        当前态重绑
                      </button>
                    </>
                  ) : null}
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={() => void onRemoveAccount(asset.account!.id)}
                    disabled={submitting || asset.account.is_active || currentLoginMatchesAccount(currentLogin, asset.account)}
                  >
                    删除
                  </button>
                </div>
              </div>
            ) : null}
          />
        </article>
      </div>

      <KeyProfileAssets
        editingKeyProfileId={editingKeyProfileId}
        keyProfileAction={keyProfileAction}
        keyProfileFormFeedback={keyProfileFormFeedback}
        keyProfileDraft={keyProfileDraft}
        onCancelEditKeyProfile={onCancelEditKeyProfile}
        onCreateKeyProfile={onCreateKeyProfile}
        onSetKeyProfileDraft={onSetKeyProfileDraft}
        submitting={submitting}
      />
    </section>
  );
}
