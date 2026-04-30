import type { Account } from "../types";
import { accountEmailLabel, accountOfficialIdLabel } from "./viewModel";

type SwitchConfirmPanelProps = {
  activeAccount: Account | null;
  canSwitch: (account: Account) => boolean;
  accountSwitchabilitySummary: (account: Account) => string;
  pendingSwitchAccount: Account;
  submitting: boolean;
  onClose: () => void;
  onConfirm: (id: number) => void | Promise<void>;
};

export function SwitchConfirmPanel({
  activeAccount,
  canSwitch,
  accountSwitchabilitySummary,
  pendingSwitchAccount,
  submitting,
  onClose,
  onConfirm,
}: SwitchConfirmPanelProps) {
  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <aside className="confirm-panel card" onClick={(event) => event.stopPropagation()}>
        <div className="card-head detail-card-head confirm-card-head">
          <div>
            <p className="eyebrow">切换确认</p>
            <h3>确认切换并采样</h3>
          </div>
          <button className="btn btn-ghost" type="button" onClick={onClose}>
            关闭
          </button>
        </div>
        <div className="binding-note confirm-note-card confirm-primary-card">
          <strong>切换路径</strong>
          <p>从：{activeAccount?.nickname ?? "未设置活跃账号"} · {accountEmailLabel(activeAccount)}</p>
          <p>到：{pendingSwitchAccount.nickname} · {accountEmailLabel(pendingSwitchAccount)}</p>
          <p>目标官方 ID：{accountOfficialIdLabel(pendingSwitchAccount)}</p>
        </div>
        <div className="binding-note confirm-note-card confirm-secondary-card">
          <strong>将执行的动作</strong>
          <p>切换后自动采样：是</p>
          <p className="confirm-passive-copy">通知中心会记录切换结果和自动采样结果。</p>
        </div>
        {!canSwitch(pendingSwitchAccount) ? (
          <div className="binding-note warning-tone confirm-note-card confirm-warning-card">
            <strong>当前不可切换</strong>
            <p>{accountSwitchabilitySummary(pendingSwitchAccount)}</p>
          </div>
        ) : null}
        <div className="row-actions confirm-action-row">
          <button className="btn btn-secondary" type="button" onClick={onClose}>
            取消
          </button>
          <button
            className="btn btn-primary"
            type="button"
            disabled={!canSwitch(pendingSwitchAccount) || submitting}
            onClick={() => void onConfirm(pendingSwitchAccount.id)}
          >
            确认切换并采样
          </button>
        </div>
      </aside>
    </div>
  );
}
