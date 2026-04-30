# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 先读这些文档

在修改这个仓库之前，先读：

1. `TASK.md`
2. `DESIGN.md`
3. `04-架构与原型文档.md`（用于理解系统级背景）

如果任务涉及未来的平台实现差异，再补充阅读：

- `CodexSwitcherMac/02-开发文档.md`
- `CodexSwitcherWin/02-开发文档.md`

## 这个仓库当前是什么

这个仓库当前**还不是**一个真实的 Tauri 应用。

它现在是：

- 以文档为先的项目
- 静态 MVP 原型
- 未来 `Tauri + React + TypeScript + Rust + SQLite` 应用的预研与落地准备区

它当前**还没有**：

- 包管理器工程配置
- 构建流水线
- lint 流水线
- 测试套件
- 真实后端
- 真实数据库接入

## 当前应该优先编辑哪些文件

对于现阶段实现任务，优先只改这三个文件：

- `prototype/index.html`
- `prototype/styles.css`
- `prototype/app.js`

除非任务明确要求把项目从静态原型迁移到下一阶段，否则**不要**创建 `src/`、`src-tauri/` 或其他真实应用目录结构。

## 常用命令

当前原型的使用方式：

- 直接打开：`open prototype/index.html`
- 或本地启动静态服务：`python3 -m http.server 8000`
- 然后访问：`http://localhost:8000/prototype/index.html`

当前仓库**没有**仓库内定义的以下命令：

- build
- lint
- 安装依赖
- tests

除非仓库后续真的引入这些工具，否则不要在回答里虚构这类命令。

## 当前阶段的实现边界

这个原型必须保持：

- 基于 mock 数据驱动
- 直接通过 `prototype/index.html` 运行
- 不依赖后端和数据库
- 便于未来迁移到 React

保持 JavaScript 极简。在当前原型中，JavaScript 只应用于：

- mock 数据
- 渲染
- 轻量 UI 状态
- 简单交互

不要在这个静态原型阶段提前引入重框架式抽象。

## 产品结构

CodexSwitcher 是一个桌面工具，核心围绕：

- 多账号监控
- 账号切换
- 接力内容生成
- 通知
- 设置与阈值管理

产品一级模块包括：

- Dashboard
- Accounts
- Handoff
- Notifications
- Settings

Dashboard 首页首屏应保持以下固定顺序：

1. 当前活跃账号总览
2. 多账号状态总览
3. 用量历史图表
4. 账号时间轴
5. 推荐动作卡

除非任务明确修改产品需求，否则不要调整这些区块顺序。

## 必须保持的设计规则

严格遵循 `DESIGN.md`。

当前阶段不可违背的规则：

- 只做浅色主题
- 整体气质要安静、克制、像桌面效率工具
- 使用文档中已有的语义色系统
- 使用文档中已有的间距 / 圆角 / 阴影语言
- 遵守 4pt 栅格系统
- 阴影必须轻
- 交互动效必须克制（`150ms–220ms`）

避免：

- 增加深色模式
- 玻璃拟态
- 赛博朋克风格
- 用大面积渐变作为主品牌表达
- 高饱和紫色主题
- 噪音感很强的动画
- 通用后台管理系统风格

## 未来真实应用的架构方向

当仓库未来进入原型之后的真实应用阶段，目标架构是：

- `Tauri` 外壳
- `React + TypeScript` UI
- `Rust` 本地服务 / 核心逻辑
- `SQLite` 持久化
- 安全凭证存储：
  - macOS 使用 `Keychain`
  - Windows 使用 `Credential Manager`

共享架构层可理解为：

1. UI 层
2. App Service 层
3. Local Router / Platform 层
4. Storage 层

平台差异应收敛在平台边界。共享业务逻辑与领域概念应尽量在 Mac 和 Windows 之间保持一致。

## 核心领域模块

长期架构围绕以下模块展开：

- `Account Manager`
- `Usage Monitor`
- `Switch Orchestrator`
- `Handoff Generator`
- `Recommendation Engine`
- `Notification Service`
- `Platform Adapter`

未来持久化实体应与文档保持一致：

- `accounts`
- `usage_snapshots`
- `switch_logs`
- `handoff_cards`
- `app_settings`
- `notifications`

## 平台差异

### macOS

- `Menubar` 是高频主入口
- 主窗口是次级但更完整的管理入口
- 视觉与交互要保留轻量、原生工具感

### Windows

- 主窗口是主要入口
- Tray 是次级快捷入口
- 布局可以比 macOS 更偏实用，但仍要保持同一套产品语言

## 这个仓库的工作方式

在这个仓库里做改动时：

- 优先做最小且完整的有效修改
- 优先在现有 prototype 文件上修改，而不是扩新结构
- 优先匹配当前原型阶段，而不是过早设计未来正式应用
- 保持文档里已有的术语和区块命名
- 当实现细节不明确时，以文档为准

如果用户请求与 `TASK.md` 或 `DESIGN.md` 冲突，要明确指出冲突，不要默默偏离既定方向。
