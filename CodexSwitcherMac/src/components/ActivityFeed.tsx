import type { ReactNode } from "react";

type ActivityFeedTone = "default" | "success" | "warning" | "danger";

type ActivityFeedItem = {
  id: string | number;
  title: string;
  body?: string;
  meta?: string;
  tone?: ActivityFeedTone;
  action?: ReactNode;
};

type ActivityFeedProps = {
  items: ActivityFeedItem[];
  emptyTitle?: string;
  emptyBody?: string;
};

function toneTag(tone: ActivityFeedTone) {
  if (tone === "success") return "healthy";
  if (tone === "warning") return "warning";
  if (tone === "danger") return "error";
  return "neutral";
}

export function ActivityFeed({
  items,
  emptyTitle = "暂无记录",
  emptyBody = "这里会显示最近发生的事件。",
}: ActivityFeedProps) {
  if (!items.length) {
    return (
      <div className="activity-feed activity-feed--empty">
        <strong>{emptyTitle}</strong>
        <p>{emptyBody}</p>
      </div>
    );
  }

  return (
    <div className="activity-feed">
      {items.map((item) => (
        <div className="activity-feed__item" key={item.id}>
          <div className="activity-feed__main">
            <div className="activity-feed__head">
              <strong>{item.title}</strong>
              {item.tone ? <span className={`status-tag ${toneTag(item.tone)}`}>{item.tone}</span> : null}
            </div>
            {item.body ? <p>{item.body}</p> : null}
            {item.meta ? <p className="helper-text">{item.meta}</p> : null}
          </div>
          {item.action ? <div className="activity-feed__action">{item.action}</div> : null}
        </div>
      ))}
    </div>
  );
}

export type { ActivityFeedItem, ActivityFeedProps, ActivityFeedTone };
