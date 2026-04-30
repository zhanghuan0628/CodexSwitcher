# CodexSwitcher

CodexSwitcher 是一个用于管理 Codex 多账号、第三方 Key、额度状态和项目会话记录的桌面工具。当前主要实现为 macOS 版本 `CodexSwitcherMac`，基于 Tauri + React + TypeScript + Rust 构建。

## 项目功能

- 管理多个 Codex 官方账号，并查看账号健康状态、登录态和额度采样结果。
- 管理第三方 API Key，将 Key 安全存入 macOS Keychain。
- 在官方账号和第三方 Key 之间切换，并自动写入当前 Codex 运行所需的 `auth.json` / `config.toml`。
- 查看账号用量、5h / 7d 使用比例、风险等级和预计恢复时间。
- 导入本机 Codex 项目会话记录，按项目、账号、Key 维度查看历史会话。
- 将导入后的 Codex 主线程同步回 Codex 侧边栏，方便在 Codex 中继续打开历史会话。
- 提供通知、切换记录、账号详情、稳定性诊断等辅助信息。

## 目录结构

```text
CodexSwitcher/
├── CodexSwitcherMac/        # macOS 桌面端主项目
│   ├── src/                 # React 前端页面与组件
│   ├── src-tauri/           # Tauri / Rust 后端逻辑
│   ├── docs/change-log/     # 变更记录
│   └── package.json
├── CodexSwitcherWin/        # Windows 版本规划文档
├── prototype/               # 早期原型页面
└── README.md
```

## 本地启动

进入 macOS 项目目录：

```bash
cd CodexSwitcherMac
```

安装依赖：

```bash
npm install
```

启动开发模式：

```bash
npm run tauri dev
```

只启动前端调试：

```bash
npm run dev
```

执行前端构建：

```bash
npm run build
```

执行 Rust 测试：

```bash
cd src-tauri
cargo test
```

## 打包安装包

在 `CodexSwitcherMac` 目录执行：

```bash
npm run tauri build -- --bundles dmg
```

生成的 DMG 通常位于：

```text
CodexSwitcherMac/src-tauri/target/release/bundle/dmg/
```

安装后如果打不开，可以在命令窗口执行：

```bash
xattr -dr com.apple.quarantine /Applications/CodexSwitcherMac.app
```

## 使用方法

1. 打开 `CodexSwitcherMac`。
2. 在账号中心绑定当前 Codex 官方登录态。
3. 如需使用第三方 Key，在 Key 管理区域新增供应商、Base URL、模型和 API Key。
4. 在仪表盘查看当前账号、额度状态、风险等级和推荐动作。
5. 需要切换账号或 Key 时，在账号中心选择目标身份并执行切换。
6. 在项目会话页面导入本机 Codex 会话记录。
7. 导入后可按项目查看会话，也可同步到 Codex 侧边栏继续使用。
8. 遇到账号异常、Keychain 不可读、登录态不匹配时，可在稳定性或账号详情页面查看诊断建议。

## 数据与安全

- CodexSwitcher 不会把账号凭证打包进安装包。
- 官方账号和第三方 Key 的敏感内容存储在用户本机 macOS Keychain。
- 应用本地数据库位于用户自己的 Application Support 目录。
- 每个用户安装后使用的是自己的 `~/.codex`、自己的 Keychain 和自己的本地数据库。
- 仓库已排除 `node_modules`、`target`、`dist`、DMG、数据库、`.env`、证书、`auth.json` 等本地或敏感文件。

## 技术栈

- Tauri 2
- Rust
- React
- TypeScript
- Vite
- SQLite
- macOS Keychain

## 注意事项

- 当前 DMG 主要面向 Apple Silicon Mac。
- 未做 Apple Developer ID 正式签名和 notarize 时，首次打开可能会被 macOS 安全策略拦截。
- 对外正式分发前，建议完成 Developer ID 签名、notarize 和发布渠道配置。
