import { SectionHeader } from "../components/SectionHeader";
import { StickyActionBar } from "../components/StickyActionBar";
import type { AppSettings } from "../types";

type SettingsPageProps = {
  autoSampleStatus: string;
  continueSamplingWhenHidden: boolean;
  onSaveSettings: () => void | Promise<void>;
  onUpdateSettings: (settings: AppSettings) => void;
  settings: AppSettings;
};

export function SettingsPage({
  autoSampleStatus,
  continueSamplingWhenHidden,
  onSaveSettings,
  onUpdateSettings,
  settings,
}: SettingsPageProps) {
  return (
    <section className="settings-workspace">
      <article className="workspace-card">
        <SectionHeader eyebrow="设置摘要" title="当前运行模式" description={autoSampleStatus} />
        <div className="accounts-workspace__summary">
          <div className="metric-tile"><span className="metric-tile__label">自动刷新</span><strong className="metric-tile__value">{settings.enable_auto_refresh ? "开启" : "关闭"}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">自动采样</span><strong className="metric-tile__value">{settings.enable_auto_sampling ? "开启" : "关闭"}</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">项目会话</span><strong className="metric-tile__value">启用</strong></div>
          <div className="metric-tile"><span className="metric-tile__label">隐藏后采样</span><strong className="metric-tile__value">{continueSamplingWhenHidden ? "继续" : "停止"}</strong></div>
        </div>
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="调度与阈值" title="基础设置" />
        <div className="form-grid settings-form-grid">
          <label>
            低阈值
            <input type="number" value={settings.warn_threshold_low} onChange={(event) => onUpdateSettings({ ...settings, warn_threshold_low: Number(event.currentTarget.value) })} />
          </label>
          <label>
            中阈值
            <input type="number" value={settings.warn_threshold_mid} onChange={(event) => onUpdateSettings({ ...settings, warn_threshold_mid: Number(event.currentTarget.value) })} />
          </label>
          <label>
            高阈值
            <input type="number" value={settings.warn_threshold_high} onChange={(event) => onUpdateSettings({ ...settings, warn_threshold_high: Number(event.currentTarget.value) })} />
          </label>
          <label>
            自动采样间隔（秒）
            <input type="number" min={10} value={settings.check_interval} onChange={(event) => onUpdateSettings({ ...settings, check_interval: Number(event.currentTarget.value) })} />
          </label>
        </div>
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="工作流开关" title="日常自动化" />
        <div className="settings-toggle-group">
          <div className="toggle-list settings-toggle-list">
            {[
              ["自动刷新状态", "enable_auto_refresh"],
              ["自动真实采样", "enable_auto_sampling"],
              ["官方扩容优先", "prefer_official_upgrade"],
            ].map(([label, key]) => (
              <label key={key} className="toggle-item settings-toggle-item">
                <span>{label}</span>
                <input
                  type="checkbox"
                  checked={settings[key as keyof AppSettings] as boolean}
                  onChange={(event) => onUpdateSettings({ ...settings, [key]: event.currentTarget.checked })}
                />
              </label>
            ))}
          </div>
        </div>
      </article>

      <article className="workspace-card">
        <SectionHeader eyebrow="运行方式" title="窗口与启动配置" />
        <div className="settings-toggle-group">
          <div className="toggle-list settings-toggle-list">
            {[
              ["开机启动", "launch_at_login"],
              ["仅 Menubar 模式", "menu_bar_only"],
            ].map(([label, key]) => (
              <label key={key} className="toggle-item settings-toggle-item">
                <span>{label}</span>
                <input
                  type="checkbox"
                  checked={settings[key as keyof AppSettings] as boolean}
                  onChange={(event) => onUpdateSettings({ ...settings, [key]: event.currentTarget.checked })}
                />
              </label>
            ))}
            <label className="toggle-item settings-toggle-item">
              <span>关闭窗口后继续采样</span>
              <input
                type="checkbox"
                checked={continueSamplingWhenHidden}
                onChange={(event) => onUpdateSettings({ ...settings, foreground_auto_sampling_only: !event.currentTarget.checked })}
              />
            </label>
          </div>
        </div>
      </article>

      <StickyActionBar>
        <button className="btn btn-primary" type="button" onClick={() => void onSaveSettings()}>
          保存设置
        </button>
      </StickyActionBar>
    </section>
  );
}
