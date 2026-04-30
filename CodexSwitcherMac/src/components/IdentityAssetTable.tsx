import type { ReactNode } from "react";
import type { Account } from "../types";
import type { IdentityAsset } from "../pages/identityViewModel";
import {
  identityKindLabel,
  identityResetValue,
  identityUsageValue,
} from "../pages/identityViewModel";
import { accountPlanBadgeLabel } from "../pages/viewModel";

type IdentityAssetTableProps = {
  assets: IdentityAsset[];
  canSwitch: (account: Account) => boolean;
  onSelectAccount?: (account: Account) => void;
  onPrimaryAction?: (asset: IdentityAsset) => void;
  primaryActionLabel?: (asset: IdentityAsset) => string;
  renderActions?: (asset: IdentityAsset) => ReactNode;
  emptyTitle?: string;
  emptyText?: string;
};

function typeBadgeClass(asset: IdentityAsset) {
  return asset.kind === "third_party_key" ? "plan-badge plan-badge--key" : "plan-badge plan-badge--official";
}

export function IdentityAssetTable({
  assets,
  canSwitch,
  onSelectAccount,
  onPrimaryAction,
  primaryActionLabel,
  renderActions,
  emptyTitle = "暂无身份",
  emptyText = "绑定官方账号或保存 Key 后，这里会显示可切换身份。",
}: IdentityAssetTableProps) {
  if (!assets.length) {
    return (
      <div className="workspace-empty-state">
        <strong>{emptyTitle}</strong>
        <p>{emptyText}</p>
      </div>
    );
  }

  return (
    <div className={`identity-asset-table ${renderActions ? "identity-asset-table--tool" : ""}`}>
      <div className="identity-asset-table__header">
        <span>身份</span>
        <span>5h</span>
        <span>7d</span>
        <span>5h 恢复</span>
        <span>7d 恢复</span>
        <span>状态</span>
        <span>操作</span>
      </div>
      <div className="identity-asset-table__body">
        {assets.map((asset) => {
          const planBadge = asset.account ? accountPlanBadgeLabel(asset.account) : null;
          const canActivate = asset.kind === "third_party_key" || Boolean(asset.account && canSwitch(asset.account));

          return (
            <div
              className={`identity-asset-table__row ${asset.isActive ? "identity-asset-table__row--active" : ""} ${asset.isRecommended ? "identity-asset-table__row--recommended" : ""}`}
              key={asset.id}
            >
              <button
                className="identity-asset-table__identity"
                type="button"
                onClick={() => asset.account ? onSelectAccount?.(asset.account) : undefined}
              >
                <div className="account-name-with-plan">
                  <strong>{asset.title}</strong>
                  {planBadge ? <span className={`plan-badge plan-badge--${planBadge}`}>{planBadge}</span> : null}
                  <span className={typeBadgeClass(asset)}>{identityKindLabel(asset)}</span>
                </div>
                <p>{asset.subtitle}</p>
                <div className="identity-asset-table__submeta">
                  {asset.meta.map((item) => <span key={item}>{item}</span>)}
                </div>
                <div className="identity-asset-table__flags">
                  {asset.isActive ? <span className="status-tag healthy">当前</span> : null}
                  {asset.isRecommended ? <span className="status-tag neutral">推荐</span> : null}
                </div>
              </button>
              <div className="identity-asset-table__metric">{identityUsageValue(asset, "window_5h_percent")}</div>
              <div className="identity-asset-table__metric">{identityUsageValue(asset, "window_7d_percent")}</div>
              <div className="identity-asset-table__metric identity-asset-table__metric--time">
                {identityResetValue(asset, "estimated_reset_5h_at")}
              </div>
              <div className="identity-asset-table__metric identity-asset-table__metric--time">
                {identityResetValue(asset, "estimated_reset_7d_at")}
              </div>
              <div className="identity-asset-table__state">
                <span className={`status-tag ${asset.status}`}>{asset.statusLabel}</span>
              </div>
              <div className="identity-asset-table__action">
                {renderActions ? renderActions(asset) : onPrimaryAction ? (
                  <button
                    className="btn btn-secondary"
                    type="button"
                    onClick={() => onPrimaryAction(asset)}
                    disabled={!canActivate || asset.isActive}
                  >
                    {primaryActionLabel?.(asset) ?? asset.actionLabel}
                  </button>
                ) : (
                  <span className="helper-text">{primaryActionLabel?.(asset) ?? asset.actionLabel}</span>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export type { IdentityAssetTableProps };

