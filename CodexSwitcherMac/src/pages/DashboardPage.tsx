import { memo } from "react";
import { AccountIdentityCard } from "../components/AccountIdentityCard";
import { IdentityAssetTable } from "../components/IdentityAssetTable";
import { SectionHeader } from "../components/SectionHeader";
import type { Account, DashboardOverview } from "../types";
import type { IdentityAsset } from "./identityViewModel";
import {
  accountResetTime,
  buildUsageSummaryText,
  dashboardAutoSampleText,
  recommendationReasonText,
  switchButtonText,
  timelineNextActionLabel,
  usageSourceText,
} from "./viewModel";

type DashboardPageProps = {
  activeAccount: Account | null;
  activeIdentity: IdentityAsset | null;
  autoSampleStatus: string;
  canSwitch: (account: Account) => boolean;
  dashboardIdentityAssets: IdentityAsset[];
  onActivateIdentity: (asset: IdentityAsset) => void | Promise<void>;
  onGoAccounts: () => void;
  onGoHandoff: () => void;
  onRefresh: () => void | Promise<void>;
  onSampleNow: () => void | Promise<void>;
  onShowSwitchConfirm: (id: number) => void | Promise<void>;
  onOpenAccountDetail: (account: Account) => void;
  overview: DashboardOverview;
  recommendationList: string[];
  recommendedIdentity: IdentityAsset | null;
  recommendedSwitchAccount: Account | null;
  submitting: boolean;
  usageDisplay: DashboardOverview["usage_display"] | null;
};

function formatUsageMoney(value: number | null | undefined, unit: string | null | undefined) {
  if (value === null || value === undefined || Number.isNaN(value)) return "--";
  if ((unit?.trim() || "") === "额度") {
    return `${Math.round(value)} 额度`;
  }
  return `${unit?.trim() || "USD"} ${value.toFixed(2)}`;
}

function keyMetrics(asset: IdentityAsset | null) {
  const profile = asset?.profile;
  const usage = profile?.usage_summary;
  if (usage?.status !== "ready") {
    return [
      { label: "身份类型", value: "Key" },
      { label: "供应商", value: profile?.provider ?? "--" },
      { label: "模型", value: profile?.model ?? "--" },
      { label: "Key", value: profile?.masked_secret ?? "未保存" },
    ];
  }

  const firstDetail = usage.detail_items[0];
  const secondDetail = usage.detail_items[1];
  const balanceDetail = usage.usage_provider_type === "new_api"
    ? (usage.plan_name ?? undefined)
    : undefined;
  return [
    {
      label: "余额",
      value: formatUsageMoney(usage.remaining ?? usage.balance, usage.unit),
      detail: balanceDetail,
    },
    {
      label: firstDetail?.label ?? "供应商",
      value: firstDetail?.value ?? (profile?.provider ?? "--"),
    },
    {
      label: secondDetail?.label ?? "模型",
      value: secondDetail?.value ?? (profile?.model ?? "--"),
    },
    { label: "模型", value: profile?.model ?? "--", detail: profile?.provider ?? undefined },
  ];
}

function identitySummary({
  identity,
  fallbackOfficialSummary,
  autoSampleStatus,
}: {
  identity: IdentityAsset | null;
  fallbackOfficialSummary: string;
  autoSampleStatus: string;
}) {
  if (identity?.kind === "third_party_key") {
    const usage = identity.profile.usage_summary;
    return [
      `当前运行身份是 Key：${identity.profile.provider}`,
      identity.profile.model ? `模型：${identity.profile.model}` : "模型未设置",
      usage?.status === "ready"
        ? `余额：${formatUsageMoney(usage.remaining ?? usage.balance, usage.unit)}`
        : "无需官方账号采样",
    ].join(" · ");
  }
  if (!identity) return fallbackOfficialSummary;
  return [fallbackOfficialSummary, dashboardAutoSampleText(autoSampleStatus)].filter(Boolean).join(" · ");
}

function DashboardChartPanel({
  overview,
  usageDisplay,
}: {
  overview: DashboardOverview;
  usageDisplay: DashboardOverview["usage_display"] | null;
}) {
  const chartLegendAccounts = (() => {
    const seen = new Set<number>();
    return overview.chart_points
      .flatMap((point) => point.series)
      .filter((item) => {
        if (seen.has(item.account_id)) return false;
        seen.add(item.account_id);
        return true;
      })
      .slice(0, 4);
  })();
  const hasData = overview.chart_points.some((point) => point.series.length > 0);

  return (
    <article className="workspace-card dashboard-panel">
      <SectionHeader eyebrow="证据层" title="用量趋势" description={usageDisplay?.chart_helper_text ?? "查看最近趋势。"} />
      <div className="dashboard-chart-legend dashboard-chart-legend--workspace">
        {chartLegendAccounts.length ? (
          chartLegendAccounts.map((account, index) => (
            <span key={account.account_id}>
              <i className={`dot ${["green", "blue", "neutral", "gold"][index] ?? "neutral"}`} /> {account.account_name}
            </span>
          ))
        ) : (
          <span>
            <i className="dot neutral" /> 暂无真实账号趋势
          </span>
        )}
      </div>
      {hasData ? (
        <div className="chart-rows prototype-chart-surface compact-chart-surface">
          {overview.chart_points.map((point) => (
            <div key={point.label} className="chart-row compact-chart-row">
              <span>{point.label}</span>
              <div className="chart-bars">
                {point.series.map((seriesItem, index) => (
                  <div
                    key={`${point.label}-${seriesItem.account_id}`}
                    className={`bar ${["green", "blue", "neutral", "gold"][index] ?? "neutral"}`}
                    style={{ width: `${seriesItem.value}%` }}
                    title={`${seriesItem.account_name} · ${seriesItem.value}%`}
                  />
                ))}
                {point.event_label ? (
                  <div className={`chart-event ${point.event_label.includes("切换") ? "healthy" : "warning"}`}>
                    {point.event_label}
                  </div>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="workspace-empty-state">
          <strong>还没有真实趋势</strong>
          <p>{usageDisplay?.chart_helper_text ?? "先做一次真实采样。"}</p>
        </div>
      )}
    </article>
  );
}

function DashboardTimelinePanel({ overview }: { overview: DashboardOverview }) {
  return (
    <article className="workspace-card dashboard-panel">
      <SectionHeader eyebrow="证据层" title="未来时间轴" description="按账号查看未来恢复与风险变化。" />
      <div className="timeline-stack compact-stack">
        {overview.timeline.map((lane) => (
          <div key={lane.account_id} className="timeline-lane prototype-timeline-lane dashboard-timeline-lane compact-timeline-lane">
            <div className="timeline-header">
              <strong>{lane.account_name}</strong>
              <span>{lane.confidence}</span>
            </div>
            <div className="timeline-track">
              {lane.segments.map((segment, index) => (
                <div
                  key={`${lane.account_id}-${index}`}
                  className={`timeline-segment ${segment.state}`}
                  title={segment.tooltip}
                >
                  <strong>{segment.label}</strong>
                  <span>{segment.hours}h</span>
                </div>
              ))}
            </div>
            <p className="helper-text timeline-next-action">{timelineNextActionLabel(lane.next_action)}</p>
          </div>
        ))}
      </div>
    </article>
  );
}

function DashboardPageInner({
  activeAccount,
  activeIdentity,
  autoSampleStatus,
  canSwitch,
  dashboardIdentityAssets,
  onActivateIdentity,
  onGoAccounts,
  onGoHandoff,
  onRefresh,
  onSampleNow,
  onShowSwitchConfirm,
  onOpenAccountDetail,
  overview,
  recommendationList,
  recommendedIdentity,
  recommendedSwitchAccount,
  submitting,
  usageDisplay,
}: DashboardPageProps) {
  const latestSnapshot = overview.latest_snapshot;
  const currentIdentityAccount = activeIdentity?.account ?? activeAccount;
  const recommendedIdentityAccount = recommendedIdentity?.account ?? recommendedSwitchAccount;
  const currentIsKey = activeIdentity?.kind === "third_party_key";
  const recommendedIsKey = recommendedIdentity?.kind === "third_party_key";

  return (
    <section className="dashboard-workspace">
      <div className="dashboard-workspace__hero">
        <AccountIdentityCard
          title="当前身份"
          account={currentIdentityAccount}
          identity={activeIdentity}
          summary={identitySummary({
            identity: activeIdentity,
            fallbackOfficialSummary: [
              buildUsageSummaryText(usageDisplay),
              usageDisplay ? `来源：${usageSourceText(usageDisplay.source_type)}` : null,
            ].filter(Boolean).join(" · "),
            autoSampleStatus,
          })}
          metrics={currentIsKey ? keyMetrics(activeIdentity) : [
            { label: "5h 剩余", value: latestSnapshot ? `${latestSnapshot.window_5h_percent}%` : "--" },
            { label: "7d 剩余", value: latestSnapshot ? `${latestSnapshot.window_7d_percent}%` : "--" },
            { label: "5h 恢复", value: latestSnapshot?.estimated_reset_5h_at ?? (activeAccount?.is_real_session ? "未知" : "待采样") },
            { label: "7d 恢复", value: latestSnapshot?.estimated_reset_7d_at ?? (activeAccount?.is_real_session ? "未知" : "待采样") },
          ]}
          actions={
            currentIsKey ? (
              <>
                <button className="btn btn-primary" type="button" onClick={onGoAccounts}>
                  账号资产
                </button>
                <button className="btn btn-secondary" type="button" onClick={() => void onRefresh()} disabled={submitting}>
                  刷新状态
                </button>
              </>
            ) : (
              <>
              {recommendedSwitchAccount ? (
                <button
                  className="btn btn-primary"
                  type="button"
                  onClick={() => void onShowSwitchConfirm(recommendedSwitchAccount.id)}
                  disabled={submitting}
                >
                  切换到 {recommendedSwitchAccount.nickname}
                </button>
              ) : (
                <button className="btn btn-primary" type="button" onClick={() => void onRefresh()} disabled={submitting}>
                  刷新状态
                </button>
              )}
              <button className="btn btn-secondary" type="button" onClick={() => void onSampleNow()} disabled={submitting}>
                立即采样
              </button>
              <button className="btn btn-ghost" type="button" onClick={onGoAccounts}>
                账号列表
              </button>
              </>
            )
          }
        />

        <AccountIdentityCard
          title="推荐身份"
          account={recommendedIdentityAccount}
          identity={recommendedIdentity}
          badge="推荐"
          summary={recommendedIsKey
            ? `可启用 Key：${recommendedIdentity.title} · ${recommendedIdentity.profile.model ?? "未设置模型"}`
            : recommendationReasonText(recommendedSwitchAccount, overview.recommended_reason, recommendationList)}
          metrics={recommendedIsKey ? keyMetrics(recommendedIdentity) : [
            { label: "5h 剩余", value: recommendedSwitchAccount ? `${recommendedSwitchAccount.latest_snapshot?.window_5h_percent ?? "--"}%` : "--" },
            { label: "7d 剩余", value: recommendedSwitchAccount ? `${recommendedSwitchAccount.latest_snapshot?.window_7d_percent ?? "--"}%` : "--" },
            { label: "5h 恢复", value: accountResetTime(recommendedSwitchAccount, "estimated_reset_5h_at") },
            { label: "7d 恢复", value: accountResetTime(recommendedSwitchAccount, "estimated_reset_7d_at") },
          ]}
          actions={
            recommendedIdentity ? (
              <>
                <button
                  className="btn btn-secondary"
                  type="button"
                  onClick={() => void onActivateIdentity(recommendedIdentity)}
                  disabled={submitting}
                >
                  {recommendedIsKey ? "启用 Key" : "去切换"}
                </button>
                {!recommendedIsKey ? <button className="btn btn-ghost" type="button" onClick={onGoHandoff}>
                  看项目会话
                </button> : null}
              </>
            ) : undefined
          }
        />
      </div>

      <article className="workspace-card dashboard-panel">
        <SectionHeader
          eyebrow="候选身份"
          title="可切换身份列表"
          description="当前身份、推荐身份和可立即接手的官方账号或 Key 都放在这里。"
          actions={
            <button className="btn btn-ghost" type="button" onClick={onGoAccounts}>
              账号中心
            </button>
          }
        />
        <IdentityAssetTable
          assets={dashboardIdentityAssets}
          canSwitch={canSwitch}
          onSelectAccount={onOpenAccountDetail}
          onPrimaryAction={(asset) => void onActivateIdentity(asset)}
          primaryActionLabel={(asset) => asset.kind === "third_party_key"
            ? asset.actionLabel
            : asset.account && canSwitch(asset.account) ? switchButtonText(asset.account) : "查看状态"}
        />
      </article>

      <div className="dashboard-workspace__evidence">
        <DashboardChartPanel overview={overview} usageDisplay={usageDisplay} />
        <DashboardTimelinePanel overview={overview} />
      </div>
    </section>
  );
}

export const DashboardPage = memo(DashboardPageInner, (prev, next) =>
  prev.activeAccount === next.activeAccount
  && prev.autoSampleStatus === next.autoSampleStatus
  && prev.overview === next.overview
  && prev.recommendationList === next.recommendationList
  && prev.recommendedIdentity === next.recommendedIdentity
  && prev.recommendedSwitchAccount === next.recommendedSwitchAccount
  && prev.dashboardIdentityAssets === next.dashboardIdentityAssets
  && prev.submitting === next.submitting
  && prev.usageDisplay === next.usageDisplay,
);
