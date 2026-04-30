const accounts = [
  {
    name: "账号 A",
    status: "healthy",
    usage: "62%",
    reset: "14:10",
    note: "当前使用中",
  },
  {
    name: "账号 B",
    status: "healthy",
    usage: "34%",
    reset: "15:40",
    note: "推荐切换",
  },
  {
    name: "账号 C",
    status: "exhausted",
    usage: "81%",
    reset: "18:20",
    note: "额度紧张",
  },
];

const timeline = [
  {
    name: "账号 A",
    blocks: [
      ["healthy", 3],
      ["warning", 2],
      ["exhausted", 1],
    ],
  },
  {
    name: "账号 B",
    blocks: [
      ["healthy", 4],
      ["warning", 2],
    ],
  },
  {
    name: "账号 C",
    blocks: [
      ["warning", 2],
      ["exhausted", 3],
      ["unknown", 1],
    ],
  },
];

function renderAccounts() {
  const root = document.getElementById("account-list");
  root.innerHTML = accounts
    .map(
      (account) => `
        <div class="account-item">
          <div class="account-meta">
            <strong>${account.name}</strong>
            <span class="account-stats">状态：${statusText(account.status)} · ${account.note}</span>
          </div>
          <div class="account-stats">使用率：${account.usage} · 恢复时间：${account.reset}</div>
          <div class="account-actions">
            <button class="btn btn-secondary">切换</button>
            <button class="btn btn-ghost">详情</button>
          </div>
        </div>
      `
    )
    .join("");
}

function renderTimeline() {
  const root = document.getElementById("timeline-lanes");
  root.innerHTML = timeline
    .map(
      (lane) => `
        <div class="timeline-lane">
          <div class="lane-label">${lane.name}</div>
          <div class="lane-track">
            ${lane.blocks
              .map(
                ([state, size]) =>
                  `<div class="lane-block ${state}" style="--size:${size}"></div>`
              )
              .join("")}
          </div>
        </div>
      `
    )
    .join("");
}

function renderChart() {
  const root = document.getElementById("usage-chart");
  root.innerHTML = `
    <svg class="chart-line" viewBox="0 0 1000 240" preserveAspectRatio="none">
      <path d="M 20 160 C 120 120, 180 110, 260 90 S 430 60, 520 70 S 700 120, 820 110 S 920 80, 980 95"
        fill="none" stroke="#58c2a8" stroke-width="4" stroke-linecap="round"/>
      <path d="M 20 180 C 120 170, 180 150, 260 145 S 430 120, 520 130 S 700 95, 820 90 S 920 120, 980 112"
        fill="none" stroke="#6fafef" stroke-width="4" stroke-linecap="round"/>
      <circle cx="520" cy="70" r="6" fill="#f2c66d" />
      <circle cx="820" cy="110" r="6" fill="#e97b73" />
      <circle cx="900" cy="92" r="6" fill="#58c2a8" />
      <line x1="650" y1="20" x2="650" y2="220" stroke="#9db8a9" stroke-width="2" stroke-dasharray="6 6"/>
      <text x="660" y="34" fill="#8b9791" font-size="12">切换点</text>
    </svg>
  `;
}

function bindSegmentedControls() {
  document.querySelectorAll(".segmented").forEach((group) => {
    group.querySelectorAll("button").forEach((button) => {
      button.addEventListener("click", () => {
        group.querySelectorAll("button").forEach((b) => b.classList.remove("active"));
        button.classList.add("active");
      });
    });
  });
}

function statusText(status) {
  const map = {
    healthy: "健康",
    warning: "预警",
    exhausted: "不可用",
    unknown: "未知",
  };
  return map[status] || status;
}

renderAccounts();
renderTimeline();
renderChart();
bindSegmentedControls();
