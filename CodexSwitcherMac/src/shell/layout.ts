export type PageKey = "dashboard" | "accounts" | "handoff" | "notifications" | "stability" | "settings";

export type SidebarAction = {
  key: string;
  label: string;
  tone?: "primary" | "secondary" | "ghost";
  disabled?: boolean;
  onClick: () => void;
};

export type SidebarSectionData = {
  key: string;
  title: string;
  items: Array<{
    label: string;
    value?: string;
    tone?: "default" | "success" | "warning" | "danger" | "muted";
  }>;
};

export const pageTitles: Record<PageKey, string> = {
  dashboard: "仪表盘",
  accounts: "账号中心",
  handoff: "项目会话",
  notifications: "通知中心",
  stability: "发布测试",
  settings: "设置",
};
