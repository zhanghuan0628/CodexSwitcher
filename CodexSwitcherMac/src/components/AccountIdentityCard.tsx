import type { ReactNode } from "react";
import type { Account } from "../types";
import type { IdentityAsset } from "../pages/identityViewModel";
import { identityKindLabel } from "../pages/identityViewModel";
import { accountMetaLine, accountPlanBadgeLabel, accountStatusCompactText, statusText } from "../pages/viewModel";
import { MetricTile } from "./MetricTile";

type AccountIdentityMetric = {
  label: string;
  value: string;
  tone?: "default" | "success" | "warning" | "danger";
  detail?: string;
};

type AccountIdentityCardProps = {
  title: string;
  account: Account | null;
  identity?: IdentityAsset | null;
  badge?: string;
  summary: string;
  metrics: AccountIdentityMetric[];
  actions?: ReactNode;
};

function identityInitials(account: Account | null, identity?: IdentityAsset | null) {
  if (identity?.title) return identity.title.slice(0, 2).toUpperCase();
  if (!account?.nickname) return "CS";
  return account.nickname.slice(0, 2).toUpperCase();
}

export function AccountIdentityCard({
  title,
  account,
  identity,
  badge,
  summary,
  metrics,
  actions,
}: AccountIdentityCardProps) {
  const planBadge = accountPlanBadgeLabel(account);
  const displayTitle = identity?.title ?? account?.nickname ?? "未设置身份";
  const displaySubtitle = identity?.subtitle ?? (account ? accountMetaLine(account) : "等待身份初始化");
  const displayStatus = identity ? identity.statusLabel : account ? statusText[account.status] : "待确认";
  const displayStatusClass = identity?.status ?? account?.status ?? "neutral";
  const displayCompact = identity
    ? `${identityKindLabel(identity)} · ${identity.statusLabel}`
    : account
      ? accountStatusCompactText(account)
      : "等待读取";

  return (
    <article className="workspace-card account-identity-card">
      <div className="account-identity-card__head">
        <div className="account-identity-card__title">
          <p className="eyebrow">{title}</p>
          <div className="account-identity-card__identity">
            <div className="account-identity-card__mark">{identityInitials(account, identity)}</div>
            <div>
              <div className="account-name-with-plan">
                <h3>{displayTitle}</h3>
                {planBadge ? <span className={`plan-badge plan-badge--${planBadge}`}>{planBadge}</span> : null}
                {identity?.kind === "third_party_key" ? <span className="plan-badge plan-badge--key">Key</span> : null}
              </div>
              <p>{displaySubtitle}</p>
            </div>
          </div>
        </div>
        <div className="account-identity-card__tags">
          {badge ? <span className="status-tag neutral">{badge}</span> : null}
          <span className={`status-tag ${displayStatusClass}`}>
            {displayStatus}
          </span>
        </div>
      </div>

      <div className="account-identity-card__summary">
        <strong>{displayCompact}</strong>
        <p>{summary}</p>
      </div>

      <div className="account-identity-card__metrics">
        {metrics.map((metric) => (
          <MetricTile
            key={`${metric.label}-${metric.value}`}
            label={metric.label}
            value={metric.value}
            tone={metric.tone}
            detail={metric.detail}
          />
        ))}
      </div>

      {actions ? <div className="account-identity-card__actions">{actions}</div> : null}
    </article>
  );
}

export type { AccountIdentityCardProps, AccountIdentityMetric };
