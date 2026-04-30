import type { ReactNode } from "react";
import type { Account } from "../types";
import {
  accountModeLabel,
  accountEmailLabel,
  accountPlanBadgeLabel,
  accountResetTime,
  accountStatusCompactText,
  accountUsagePercent,
  statusText,
} from "../pages/viewModel";

type AccountListTableProps = {
  accounts: Account[];
  activeAccountId?: number | null;
  recommendedAccountId?: number | null;
  onSelect?: (account: Account) => void;
  onPrimaryAction?: (account: Account) => void;
  primaryActionLabel: (account: Account) => string;
  renderActions?: (account: Account) => ReactNode;
};

export function AccountListTable({
  accounts,
  activeAccountId,
  recommendedAccountId,
  onSelect,
  onPrimaryAction,
  primaryActionLabel,
  renderActions,
}: AccountListTableProps) {
  if (!accounts.length) {
    return (
      <div className="workspace-empty-state">
        <strong>暂无账号</strong>
        <p>绑定真实账号后，这里会显示可切换账号列表。</p>
      </div>
    );
  }

  return (
    <div className={`account-list-table ${renderActions ? "account-list-table--tool" : ""}`}>
      <div className="account-list-table__header">
        <span>账号</span>
        <span>5h</span>
        <span>7d</span>
        <span>5h 恢复</span>
        <span>7d 恢复</span>
        <span>状态</span>
        <span>操作</span>
      </div>
      <div className="account-list-table__body">
        {accounts.map((account) => (
          <div
            className={`account-list-table__row ${account.id === activeAccountId ? "account-list-table__row--active" : ""} ${account.id === recommendedAccountId ? "account-list-table__row--recommended" : ""}`}
            key={account.id}
          >
            <button
              className="account-list-table__account"
              type="button"
              onClick={() => onSelect?.(account)}
            >
              <div className="account-name-with-plan">
                <strong>{account.nickname}</strong>
                {accountPlanBadgeLabel(account) ? (
                  <span className={`plan-badge plan-badge--${accountPlanBadgeLabel(account)}`}>{accountPlanBadgeLabel(account)}</span>
                ) : null}
              </div>
              <p>{accountEmailLabel(account)}</p>
              <div className="account-list-table__submeta">
                <span>{accountModeLabel(account)}</span>
                <span>{accountStatusCompactText(account)}</span>
              </div>
              <div className="account-list-table__flags">
                {account.id === activeAccountId ? <span className="status-tag healthy">当前</span> : null}
                {account.id === recommendedAccountId ? <span className="status-tag neutral">推荐</span> : null}
              </div>
            </button>
            <div className="account-list-table__metric">{accountUsagePercent(account, "window_5h_percent")}</div>
            <div className="account-list-table__metric">{accountUsagePercent(account, "window_7d_percent")}</div>
            <div className="account-list-table__metric account-list-table__metric--time">
              {accountResetTime(account, "estimated_reset_5h_at")}
            </div>
            <div className="account-list-table__metric account-list-table__metric--time">
              {accountResetTime(account, "estimated_reset_7d_at")}
            </div>
            <div className="account-list-table__state">
              <span className={`status-tag ${account.status}`}>{statusText[account.status]}</span>
            </div>
            <div className="account-list-table__action">
              {renderActions ? renderActions(account) : onPrimaryAction ? (
                <button className="btn btn-secondary" type="button" onClick={() => onPrimaryAction(account)}>
                  {primaryActionLabel(account)}
                </button>
              ) : (
                <span className="helper-text">{primaryActionLabel(account)}</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export type { AccountListTableProps };
