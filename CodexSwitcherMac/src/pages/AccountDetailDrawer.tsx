import { AccountIdentityCard } from "../components/AccountIdentityCard";
import { ActivityFeed } from "../components/ActivityFeed";
import { SectionHeader } from "../components/SectionHeader";
import type { Account, AccountDetail, NotificationItem, SessionRecord, SwitchLog } from "../types";
import {
  accountEmailLabel,
  accountOfficialIdLabel,
  accountPlanBadgeLabel,
  accountResetTime,
  accountUsagePair,
  accountUsagePercent,
  statusLabel,
  statusText,
  switchButtonText,
  switchHintText,
} from "./viewModel";

type AccountDetailDrawerProps = {
  account: Account;
  accountDetail: AccountDetail | null;
  authStateText: Record<string, string>;
  canSwitch: (account: Account) => boolean;
  hasAuthIssue: (account: Account) => boolean;
  accountSwitchabilitySummary: (account: Account) => string;
  notifications: NotificationItem[];
  sessions: SessionRecord[];
  submitting: boolean;
  switchLogs: SwitchLog[];
  onBeginRepairFlow: (id: number) => void | Promise<void>;
  onClose: () => void;
  onCopyDiagnostic: () => void | Promise<void>;
  onLoadDetail: (id: number) => void | Promise<void>;
  onOpenSessionsPage: () => void;
  onSampleNow: () => void | Promise<void>;
  onSwitchAccount: (id: number) => void | Promise<void>;
};

export function AccountDetailDrawer({
  account,
  accountDetail,
  authStateText,
  canSwitch,
  hasAuthIssue,
  accountSwitchabilitySummary,
  notifications,
  sessions,
  submitting,
  switchLogs,
  onBeginRepairFlow,
  onClose,
  onCopyDiagnostic,
  onLoadDetail,
  onOpenSessionsPage,
  onSampleNow,
  onSwitchAccount,
}: AccountDetailDrawerProps) {
  const recentSwitches = accountDetail?.recent_switches.length
    ? accountDetail.recent_switches
    : switchLogs.filter((log) => log.from_account_id === account.id || log.to_account_id === account.id).slice(0, 10);
  const recentSessions = accountDetail?.recent_sessions.length ? accountDetail.recent_sessions : sessions;
  const recentNotifications = accountDetail?.recent_notifications.length ? accountDetail.recent_notifications : notifications;

  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <aside className="account-drawer workspace-card" onClick={(event) => event.stopPropagation()}>
        <div className="detail-card-head">
          <div>
            <p className="eyebrow">账号详情</p>
            <div className="account-name-with-plan">
              <h3>{account.nickname}</h3>
              {accountPlanBadgeLabel(account) ? (
                <span className={`plan-badge plan-badge--${accountPlanBadgeLabel(account)}`}>{accountPlanBadgeLabel(account)}</span>
              ) : null}
            </div>
          </div>
          <button className="btn btn-ghost" type="button" onClick={onClose}>
            关闭
          </button>
        </div>

        <AccountIdentityCard
          title="当前查看账号"
          account={account}
          summary={accountSwitchabilitySummary(account)}
          metrics={[
            { label: "邮箱", value: accountEmailLabel(account) },
            { label: "账号 ID", value: accountOfficialIdLabel(account) },
            { label: "5h / 7d", value: accountUsagePair(account) },
            { label: "5h 恢复", value: accountResetTime(account, "estimated_reset_5h_at") },
            { label: "7d 恢复", value: accountResetTime(account, "estimated_reset_7d_at") },
          ]}
          actions={
            <>
              <button
                className="btn btn-primary"
                type="button"
                disabled={!canSwitch(account) || submitting}
                onClick={() => void onSwitchAccount(account.id)}
              >
                {switchButtonText(account)}
              </button>
              {hasAuthIssue(account) ? (
                <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onBeginRepairFlow(account.id)}>
                  重新登录并重绑
                </button>
              ) : null}
              <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onSampleNow()}>
                立即采样
              </button>
            </>
          }
        />

        <div className="account-detail-grid">
          <article className="binding-note account-detail-card">
            <SectionHeader eyebrow="身份与状态" title="账号基础信息" />
            <p>授权状态：{authStateText[account.auth_state] ?? account.auth_state}</p>
            <p>账号状态：{statusText[account.status]}</p>
            <p>默认账号：{account.is_default ? "是" : "否"}</p>
            <p>当前活跃：{account.is_active ? "是" : "否"}</p>
            <p>{switchHintText(account)}</p>
          </article>

          <article className="binding-note account-detail-card">
            <SectionHeader eyebrow="高级信息" title="Keychain 与绑定快照" />
            <p>绑定方式：{account.binding_kind}</p>
            <p>会话引用：{account.session_ref || "--"}</p>
            <p>账号 Key：{account.account_key || "--"}</p>
            <p>Keychain：{accountDetail?.keychain_readable ? "可读" : "不可读或尚未加载"}</p>
            <p>绑定快照：{accountDetail?.bound_snapshot_summary ?? "--"}</p>
            <div className="row-actions compact account-detail-action-row">
              <button className="btn btn-secondary" type="button" onClick={() => void onLoadDetail(account.id)}>
                刷新详情
              </button>
              <button className="btn btn-secondary" type="button" onClick={() => void onCopyDiagnostic()}>
                复制诊断信息
              </button>
            </div>
          </article>

          <article className={`binding-note account-detail-card ${accountDetail?.last_failure_reason ? "warning-tone account-detail-warning-card" : ""}`.trim()}>
            <SectionHeader eyebrow="最近状态" title="失败与预警说明" />
            <p>{accountDetail?.last_failure_reason ?? "暂无失败或预警记录"}</p>
            <p>最近检测：{account.last_check_time ?? "未检测"}</p>
            <p>最近校验：{account.last_verified_at ?? "未校验"}</p>
            <p>最新剩余：5h {accountUsagePercent(account, "window_5h_percent")} · 7d {accountUsagePercent(account, "window_7d_percent")}</p>
          </article>

          <article className="binding-note account-detail-card">
            <SectionHeader eyebrow="健康轨迹" title="账号健康时间线" />
            {(accountDetail?.health_timeline ?? []).length ? (
              <div className="mini-health-track">
                {(accountDetail?.health_timeline ?? []).map((segment, index) => (
                  <span
                    key={`${segment.label}-${index}`}
                    className={`mini-health-dot ${segment.state}`}
                    title={segment.tooltip}
                  >
                    {segment.label}
                  </span>
                ))}
              </div>
            ) : (
              <p className="account-detail-passive-copy">暂无健康时间线</p>
            )}
          </article>
        </div>

        <div className="account-detail-feed-grid">
          <article className="workspace-card account-detail-feed-card">
            <SectionHeader eyebrow="采样记录" title="最近 10 次采样" />
            <ActivityFeed
              items={(accountDetail?.recent_snapshots ?? []).map((snapshot) => ({
                id: `${snapshot.account_id}-${snapshot.sample_time}`,
                title: snapshot.sample_time,
                body: `5h 剩余 ${snapshot.window_5h_percent}% · 7d 剩余 ${snapshot.window_7d_percent}%`,
                meta: statusLabel(snapshot.risk_level),
              }))}
              emptyTitle="暂无采样记录"
              emptyBody="做一次采样后，这里会显示最近快照。"
            />
          </article>

          <article className="workspace-card account-detail-feed-card">
            <SectionHeader eyebrow="切换记录" title="最近 10 次切换" />
            <ActivityFeed
              items={recentSwitches.map((log) => ({
                id: log.id,
                title: `${log.created_at} · ${log.result}`,
                body: log.reason,
                meta: `切换到账号 #${log.to_account_id}`,
              }))}
              emptyTitle="暂无切换记录"
              emptyBody="发生切换后，这里会保留最近动作。"
            />
          </article>
        </div>

        <div className="account-detail-feed-grid">
          <article className="workspace-card account-detail-feed-card">
            <SectionHeader
              eyebrow="项目会话"
              title="最近会话"
              actions={
                <button className="btn btn-secondary" type="button" onClick={onOpenSessionsPage}>
                  查看项目会话
                </button>
              }
            />
            <ActivityFeed
              items={recentSessions.map((record) => ({
                id: record.id,
                title: record.title || "未命名会话",
                body: record.summary || "暂无摘要",
                meta: `${record.project_name} · ${record.updated_at}`,
              }))}
              emptyTitle="暂无会话记录"
              emptyBody="导入或创建项目会话后，这里会显示最近记录。"
            />
          </article>

          <article className="workspace-card account-detail-feed-card">
            <SectionHeader eyebrow="通知关联" title="最近通知" />
            <ActivityFeed
              items={recentNotifications.map((item) => ({
                id: item.id,
                title: item.title,
                body: item.message,
                meta: item.created_at,
              }))}
              emptyTitle="暂无关联通知"
              emptyBody="账号相关通知会同步保留在这里。"
            />
          </article>
        </div>
      </aside>
    </div>
  );
}
