# CodexSwitcher Mac 真实账号绑定与切换方案

## 1. 目标

本方案用于把 `CodexSwitcherMac/` 固定到“Codex 官方登录 + 真实 Codex 账号绑定 + 真实账号切换 + 真实授权修复 + 真实数据驱动”的可用版本。

第一阶段范围明确为：

- 只支持 `Codex`
- 绑定方式优先采用 **Codex CLI 官方登录流程**：应用只负责发起 `codex login`、读取 `~/.codex/auth.json` 的登录态摘要，并把敏感会话材料放入 macOS Keychain
- 不做破解登录态或规避风控能力
- 不依赖不稳定的外部隐式接口作为唯一数据源

## 2. 当前状态与问题

当前项目已经从“演示账号模型”推进到“真实 Codex 账号第一阶段”：

- 前端主入口是“开始官方登录”和“绑定当前已登录账号”
- Rust 侧发起的是 `codex login`，不是任何其他供应商登录流程
- 绑定校验只接受 `auth_mode = chatgpt` 的 Codex 官方账号登录态
- `switch_account` 会执行真实会话写入、切换后校验、失败回滚与日志记录
- Dashboard / Timeline / Recommendation 只展示真实快照或 `unknown`，不再补假数据

当前仍需继续补强的是：

- Codex 官方额度读取链路暂不稳定，拿不到真实额度时必须显示 `unknown`
- Rust 代码仍集中在 `src-tauri/src/lib.rs`，后续需要模块化
- 多账号切换依赖本地 Codex CLI 登录态格式，必须持续保守校验

## 3. 真实版的定义边界

### 3.1 什么叫“真实绑定”

真实绑定指：

1. 用户通过官方支持的认证方式完成 Codex 登录
2. 应用获取该账号对应的真实会话引用或凭证材料
3. 敏感数据安全写入 macOS Keychain
4. SQLite 只保存账号元数据、状态和安全引用
5. 重启应用后，仍可恢复已绑定账号列表并执行验证

### 3.2 什么叫“真实切换”

真实切换指：

1. 应用读取目标账号的真实会话数据
2. 将当前活动会话切换为目标账号
3. 切换后立即做一次真实校验
4. 只有校验通过，才把该账号标记为 active
5. 如果切换失败，必须回滚到原账号会话，并记录失败日志与通知

### 3.3 什么叫“真实授权修复”

真实授权修复指：

1. 发现账号凭证失效、过期或不可用
2. 应用触发重新登录或重新绑定流程
3. 只有真实校验成功后，才恢复状态
4. 不能仅通过修改本地状态字段伪装为“已修复”

## 4. 事实来源与存储边界

### 4.1 SQLite 的职责

SQLite 只负责保存非敏感信息：

- 账号索引与展示信息
- 当前活跃账号标记
- 最近验证时间
- 授权状态摘要
- 切换日志
- 通知记录
- 用量快照与 `unknown` 兜底结果
- 设置项

SQLite **不应保存**：

- token 明文
- cookie 明文
- secret 明文
- 可直接复用的敏感会话原文

### 4.2 Keychain 的职责

macOS Keychain 是唯一敏感凭证存储：

- 保存账号真实凭证材料
- 保存切换所需的敏感会话内容
- 删除账号时同步删除对应凭证
- 所有读取都只能在 Rust 侧完成

前端不得接触明文凭证。

### 4.3 真实能力的事实来源

真实绑定与切换必须优先基于：

- 官方支持的登录流程
- 官方支持的 CLI / 会话管理入口
- 本地已存在的官方登录态

如果缺少稳定官方接口：

- 可以采用“应用内引导 + 官方流程完成 + 应用回收结果绑定”的方式
- 不能把扫描本地隐式配置文件当作唯一方案
- 不能宣称已经实现“纯应用内完全接管登录”

## 5. 数据模型调整方向

当前 `accounts` 需要从“展示型账号卡片”改为“真实会话槽位”。

建议在保留现有基本字段基础上，新增或替换以下语义字段：

- `account_key`：账号唯一稳定标识
- `binding_kind`：绑定方式，例如 `codex_cli` / `embedded_login`
- `session_ref`：指向 Keychain 中敏感数据的稳定引用
- `profile_ref`：本地配置或官方 profile 的引用
- `last_verified_at`：最近一次真实验证时间
- `auth_state`：真实授权状态摘要
- `status`：界面展示状态，如 `healthy` / `warning` / `auth_invalid` / `error`

其中：

- SQLite 保存引用与状态摘要
- Keychain 保存敏感内容本体

## 6. API 与命令边界调整

当前 API 仍围绕本地 CRUD，需要改为围绕真实绑定与真实切换。

### 6.1 应废弃为主路径的接口

以下接口不应再作为主要用户路径：

- `create_account(provider, nickname)`
- 仅修改状态字段的 `repair_account_auth`
- 仅更新 `is_active` 的 `switch_account`

### 6.2 建议新增接口

前端 `src/lib/api.ts` 与 Rust command 建议新增：

- `start_codex_login_flow`
- `complete_codex_login_binding`
- `list_bound_accounts`
- `verify_bound_account`
- `rebind_account`
- `switch_account`（重写为真实切换）
- `remove_bound_account`
- `get_active_account_binding`

## 7. 前端交互调整

Accounts 页面应从“手工新增账号”改为“真实绑定入口”。

### 7.1 新的主操作

建议提供以下按钮：

- 登录并绑定 Codex 账号
- 验证账号
- 重新登录
- 切换为当前账号
- 删除账号

### 7.2 不再保留的主流程

以下流程应从主界面移除或降级：

- 手工填写 provider 创建账号
- 手工设置 auth_state 作为修复手段
- 只根据本地记录判断切换已成功

### 7.3 页面状态要求

前端需要明确展示：

- 当前账号是否真实绑定成功
- 当前状态来自真实验证还是 unknown 兜底
- 切换中 / 验证中 / 重新登录中的加载态
- 切换失败后的回滚结果

## 8. Rust 侧实现职责

Rust 侧需要承担真实会话相关逻辑，不把敏感流程暴露给前端。

### 8.1 建议拆分模块

建议至少新增以下模块：

- `keychain/`：Keychain 读写封装
- `account_manager/`：账号绑定、验证、删除
- `switch_orchestrator/`：切换执行、校验、失败回滚
- `platform/`：与本机 CLI / 配置 / 会话入口交互
- `db/`：SQLite 访问与模型映射

`src-tauri/src/lib.rs` 只保留：

- 应用状态注入
- command 注册
- 模块装配

### 8.2 Keychain 接口建议

建议封装统一接口：

- `save_account_secret(account_key, payload)`
- `load_account_secret(account_key)`
- `delete_account_secret(account_key)`

### 8.3 切换执行链路

`switch_account(target_account_id)` 需要改造成：

1. 读取当前账号会话引用
2. 读取目标账号 Keychain 会话数据
3. 执行真实切换
4. 立即做真实校验
5. 成功后写入 active 状态与成功日志
6. 失败则回滚旧会话，写失败日志与通知

## 9. Dashboard / Notifications / Menubar 的真实化要求

真实切换做完后，展示层也必须逐步切换到真实数据。

### 9.1 Dashboard

优先展示：

- 当前真实活跃账号
- 最近真实验证结果
- 真实切换日志
- 真实通知结果

如果某些额度数据暂时拿不到稳定来源：

- 可以标注为 `unknown`
- 不能伪装成真实采样

### 9.2 Notifications

通知必须基于真实事件：

- 切换成功
- 切换失败并已回滚
- 账号需要重新认证
- 验证失败

### 9.3 Menubar

Menubar 中展示的当前账号和可切换账号，应来自真实绑定列表，而不是 seed 数据。

## 10. 推荐实施顺序

### 阶段 0：先落文档

先在项目中保存本方案文档，作为后续改造基线。

### 阶段 1：重构数据模型

修改：

- `src/types.ts`
- `src-tauri/src/lib.rs`
- SQLite schema

目标：

- 把账号模型改为真实会话槽位模型

### 阶段 2：接入 Keychain

目标：

- 敏感凭证从 SQLite 中彻底移出
- 建立统一 Keychain 读写封装

### 阶段 3：打通真实绑定

目标：

- 从“创建本地账号卡片”改成“登录并绑定真实 Codex 账号”

### 阶段 4：打通真实切换

目标：

- 从“更新 active 状态”改成“切换真实会话 + 真实校验 + 回滚”

### 阶段 5：打通真实授权修复

目标：

- 从“本地状态恢复”改成“重新登录 / 重新绑定”

### 阶段 6：清理 seed 依赖

目标：

- Dashboard / Notifications / Menubar 以真实数据为主
- 对无法实时获取的指标明确标注 unknown

## 11. 风险与处理原则

### 风险 1：缺少稳定官方多账号切换接口

这是最大不确定性。

处理原则：

- 优先寻找官方稳定 CLI / 登录 / profile 能力
- 如果没有，不伪造能力边界
- 采用“应用内引导 + 官方流程完成 + 应用绑定结果”的方案

### 风险 2：本机会话文件格式不稳定

处理原则：

- 不把扫描本地隐式文件作为唯一数据源
- 可以作为辅助发现，不可作为唯一事实来源

### 风险 3：真实额度数据不完整

处理原则：

- 界面明确区分真实值与未知值
- 不得用本地推断或演示数据伪装为真实采样

### 风险 4：当前代码集中在单文件

处理原则：

- 实现真实能力时同步做最小必要拆分
- 避免继续把核心逻辑堆进 `src/App.tsx` 与 `src-tauri/src/lib.rs`

## 12. 关键改造文件

当前必须修改：

- `src/App.tsx`
- `src/App.css`
- `src/lib/api.ts`
- `src/types.ts`
- `src-tauri/src/lib.rs`

建议新增：

- `src/pages/Accounts.tsx`
- `src/modules/accounts/*`
- `src/modules/switch/*`
- `src-tauri/src/keychain/*`
- `src-tauri/src/account_manager/*`
- `src-tauri/src/switch_orchestrator/*`
- `src-tauri/src/platform/*`
- `src-tauri/src/db/*`

## 13. 验收标准

达到以下条件后，才可认为第一阶段“真实可用”成立：

1. 用户可以在应用中发起 Codex 登录并完成真实绑定
2. 重启应用后，已绑定账号仍可恢复
3. 账号切换会真实改变当前使用的会话
4. 切换后会立即执行真实校验
5. 切换失败会回滚并留下日志与通知
6. 授权修复走真实重新登录或重新绑定流程
7. 删除账号会同步清理 SQLite 索引与 Keychain 数据
8. Menubar / Dashboard / Notifications 展示的账号状态来自真实绑定结果

## 14. 当前结论

从当前代码状态看，`CodexSwitcherMac/` 已进入真实 Codex 账号第一阶段：登录入口、绑定、校验、切换、回滚、Keychain 存储和演示数据清理已经按 Codex 官方登录链路收敛。后续所有相关开发都应以本方案为准，继续补强真实额度读取、模块化和回归测试，不得再引入其他供应商登录、演示账号或本地假数据作为主路径。
