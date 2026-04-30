import { useMemo } from "react";
import { ActivityFeed } from "../components/ActivityFeed";
import { SectionHeader } from "../components/SectionHeader";
import type { Account, NotificationItem, NotificationSourceType } from "../types";
import { filterNotifications, levelClass, notificationActionText, notificationFilters, notificationSourceText } from "./viewModel";

type NotificationFilter = "all" | NotificationSourceType;

type NotificationsPageProps = {
  notificationAccountFilter: number | "all";
  notificationFilter: NotificationFilter;
  notifications: NotificationItem[];
  onOpenAccountDetail: (account: Account) => void;
  onSetNotificationAccountFilter: (value: number | "all") => void;
  onSetNotificationFilter: (value: NotificationFilter) => void;
  realAccounts: Account[];
  relatedAccountForNotification: (item: NotificationItem) => Account | null;
  settingsPreferOfficialUpgrade: boolean;
};

export function NotificationsPage({
  notificationAccountFilter,
  notificationFilter,
  notifications,
  onOpenAccountDetail,
  onSetNotificationAccountFilter,
  onSetNotificationFilter,
  realAccounts,
  relatedAccountForNotification,
  settingsPreferOfficialUpgrade,
}: NotificationsPageProps) {
  const filteredNotifications = useMemo(
    () =>
      filterNotifications({
        notificationAccountFilter,
        notificationFilter,
        notifications,
        relatedAccountForNotification,
      }),
    [notificationAccountFilter, notificationFilter, notifications, relatedAccountForNotification],
  );

  return (
    <section className="notifications-workspace">
      <article className="workspace-card">
        <SectionHeader
          eyebrow="通知策略"
          title="事件工作台"
          description={
            settingsPreferOfficialUpgrade
              ? "当前会优先提醒官方扩容，再考虑账号切换。"
              : "当前会更积极推荐账号切换作为主要动作。"
          }
        />
        <div className="accounts-workspace__summary">
          <div className="metric-tile"><span className="metric-tile__label">通知总数</span><strong className="metric-tile__value">{notifications.length}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">当前筛选</span><strong className="metric-tile__value">{notificationFilters.find((item) => item.key === notificationFilter)?.label ?? "全部"}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">账号范围</span><strong className="metric-tile__value">{notificationAccountFilter === "all" ? "全部账号" : `账号 #${notificationAccountFilter}`}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">筛选结果</span><strong className="metric-tile__value">{filteredNotifications.length}</strong></div>
        </div>
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="筛选器" title="按来源与账号缩小范围" />
        <div className="filter-row notifications-filter-row">
          {notificationFilters.map((filter) => (
            <button
              className={`btn ${notificationFilter === filter.key ? "btn-primary" : "btn-secondary"}`}
              key={filter.key}
              type="button"
              onClick={() => onSetNotificationFilter(filter.key)}
            >
              {filter.label}
            </button>
          ))}
        </div>
        <div className="filter-row notifications-filter-row">
          <button
            className={`btn ${notificationAccountFilter === "all" ? "btn-primary" : "btn-secondary"}`}
            type="button"
            onClick={() => onSetNotificationAccountFilter("all")}
          >
            全部账号
          </button>
          {realAccounts.map((account) => (
            <button
              className={`btn ${notificationAccountFilter === account.id ? "btn-primary" : "btn-secondary"}`}
              key={account.id}
              type="button"
              onClick={() => onSetNotificationAccountFilter(account.id)}
            >
              {account.nickname}
            </button>
          ))}
        </div>
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="事件时间线" title="最近通知事件" description="按时间查看通知并直接跳回相关账号。" />
        <ActivityFeed
          items={filteredNotifications.map((item) => {
            const relatedAccount = relatedAccountForNotification(item);
            return {
              id: item.id,
              title: item.title,
              body: item.message,
              meta: `${notificationSourceText(item.source_type)} · ${item.action_type} · ${item.created_at}`,
              tone: item.level === "success" ? "success" : item.level === "warning" ? "warning" : item.level === "error" ? "danger" : "default",
              action: (
                <div className="row-actions compact notifications-event-actions">
                  <span className={`status-tag ${levelClass(item.level)}`}>{item.level}</span>
                  {relatedAccount ? (
                    <button className="btn btn-secondary" type="button" onClick={() => onOpenAccountDetail(relatedAccount)}>
                      账号详情
                    </button>
                  ) : null}
                  <span className="helper-text">{notificationActionText(item)}</span>
                </div>
              ),
            };
          })}
          emptyTitle="暂无通知记录"
          emptyBody="发生阈值预警、切换结果、设置变更或项目会话事件后，这里会保留记录。"
        />
      </article>
    </section>
  );
}
