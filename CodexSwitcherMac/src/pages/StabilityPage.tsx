import { ActivityFeed } from "../components/ActivityFeed";
import { SectionHeader } from "../components/SectionHeader";
import type { Account, CleanupPreview, NotificationItem, ReleaseDiagnostic, StartupHealth } from "../types";
import { levelClass, statusText } from "./viewModel";

type RegressionCheck = {
  label: string;
  passed: boolean;
  detail: string;
};

type StabilityPageProps = {
  cleanupPreview: CleanupPreview | null;
  notifications: NotificationItem[];
  onCleanupDebugData: () => void | Promise<void>;
  onLoadReleaseDiagnostic: () => void | Promise<void>;
  onLoadStartupHealth: () => void | Promise<void>;
  regressionChecks: RegressionCheck[];
  releaseDiagnostic: ReleaseDiagnostic | null;
  relatedAccountForNotification: (item: NotificationItem) => Account | null;
  startupHealth: StartupHealth | null;
  submitting: boolean;
};

export function StabilityPage({
  cleanupPreview,
  notifications,
  onCleanupDebugData,
  onLoadReleaseDiagnostic,
  onLoadStartupHealth,
  regressionChecks,
  releaseDiagnostic,
  relatedAccountForNotification,
  startupHealth,
  submitting,
}: StabilityPageProps) {
  return (
    <section className="stability-workspace">
      <div className="stability-workspace__hero">
        <article className="workspace-card">
          <SectionHeader
            eyebrow="启动健康检查"
            title={startupHealth ? (startupHealth.healthy ? "启动检查通过" : "启动存在风险") : "尚未运行检查"}
            actions={
              <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onLoadStartupHealth()}>
                重新检查
              </button>
            }
          />
          <ActivityFeed
            items={(startupHealth?.checks ?? []).map((check) => ({
              id: check.label,
              title: check.label,
              body: check.detail,
              tone: check.ok ? "success" : "danger",
            }))}
            emptyTitle="尚未运行启动健康检查"
            emptyBody="点击重新检查后，这里会展示启动环境结果。"
          />
        </article>

        <article className="workspace-card">
          <SectionHeader
            eyebrow="发布诊断"
            title="环境与账号健康"
            actions={
              <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onLoadReleaseDiagnostic()}>
                刷新发布诊断
              </button>
            }
          />
          {releaseDiagnostic ? (
            <ActivityFeed
              items={releaseDiagnostic.accounts.map((item) => ({
                id: item.account_id,
                title: item.nickname,
                body: item.advice,
                meta: `${item.email ?? item.profile_ref ?? "未读取身份"} · Keychain ${item.keychain_readable ? "可读" : "不可读"} · ${statusText[item.status]}`,
                tone: item.keychain_readable ? "success" : "warning",
              }))}
            />
          ) : (
            <div className="workspace-empty-state">
              <strong>尚未运行诊断</strong>
              <p>点击“刷新发布诊断”后，会检查 Codex CLI、当前登录态、Keychain 凭证和最近采样/切换。</p>
            </div>
          )}
        </article>
      </div>

      <article className="workspace-card">
        <SectionHeader eyebrow="真实多账号回归" title="测试清单" />
        <ActivityFeed
          items={regressionChecks.map((item) => ({
            id: item.label,
            title: item.label,
            body: item.detail,
            tone: item.passed ? "success" : "warning",
          }))}
        />
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="测试日志" title="最近真实事件" />
        <ActivityFeed
          items={notifications.slice(0, 10).map((item) => {
            const relatedAccount = relatedAccountForNotification(item);
            return {
              id: item.id,
              title: item.title,
              body: item.message,
              meta: `${item.created_at} · ${item.action_type} · ${relatedAccount?.nickname ?? "未关联账号"}`,
              tone: item.level === "success" ? "success" : item.level === "warning" ? "warning" : item.level === "error" ? "danger" : "default",
              action: <span className={`status-tag ${levelClass(item.level)}`}>{item.level}</span>,
            };
          })}
        />
      </article>

      <article className="workspace-card stability-workspace__cleanup">
        <SectionHeader
          eyebrow="历史清理"
          title="旧调试数据"
          actions={
            <>
              <button className="btn btn-secondary" type="button" disabled={submitting} onClick={() => void onLoadReleaseDiagnostic()}>
                重新统计
              </button>
              <button className="btn btn-danger" type="button" disabled={submitting} onClick={() => void onCleanupDebugData()}>
                清理历史调试数据
              </button>
            </>
          }
        />
        <div className="accounts-workspace__summary">
          <div className="metric-tile"><span className="metric-tile__label">旧上下文</span><strong className="metric-tile__value">已下线</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">旧通知</span><strong className="metric-tile__value">{cleanupPreview?.old_notification_count ?? "--"}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">项目会话</span><strong className="metric-tile__value">独立</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">清理范围</span><strong className="metric-tile__value">仅旧调试数据</strong></div>
        </div>
      </article>
    </section>
  );
}
