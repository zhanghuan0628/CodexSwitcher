# 按 Key 隔离 Codex 会话可见性

- 变更背景：所有第三方 Key 和官方账号共用 Codex 的 `state_5.sqlite`。切换身份后，旧实现会把所有已导入线程解除归档；但当前 `config.toml` 只包含当前 Key 的 provider，导致其他 Key 的会话仍出现在侧边栏且打开时报 `Model provider '<name>' not found`。
- 本次变更内容：第三方 Key 的 Codex provider 内部标识改为稳定的 `codexswitcher-key-<profile-id>`，不再使用可重复的供应商名称；界面显示名仍使用用户设置的供应商名称。身份切换、当前 Key 编辑、应用启动和会话列表刷新时，只显示当前 Key 对应的线程；切到官方账号时只显示 `openai` 线程。非当前身份线程会保留在本地但设为归档，不会删除。
- Codex 重载：最新版 Codex 会在进程内缓存当前 provider 与侧栏线程。Switcher 完成身份切换和可见性写入后，如果 Codex 正在运行，会自动退出并重新打开，让新配置和会话过滤同时生效。
- 会话归档协议：新版 `history_mode=paginated` 会话会在 Codex 启动时从 `sessions` 目录重新发现，仅修改 `state_5.sqlite.archived` 不足以隐藏。Switcher 现在通过 Codex app-server 的 `thread/archive` 和 `thread/unarchive` 官方协议移动 rollout 文件并更新索引，避免其他 Key 或官方会话在重启后再次出现。旧版 Switcher 创建在 `sessions/codexswitcher-imported` 下的非标准 rollout 文件会先迁移为 Codex 认可的日期目录和文件名，再参与归档；单条会话失败不会再跳过后续整批会话。
- 无效索引处理：如果 `state_5.sqlite` 仍有 thread 记录，但对应 rollout 文件已经不存在，该记录会保持隐藏。此类记录无法恢复或打开，继续显示只会形成不可点击的侧栏死记录。
- 涉及范围：第三方 Key 运行配置、Codex thread 写回、已导入线程的 provider 回填、Codex 侧边栏可见性。
- 是否影响配置：会在下次启用第三方 Key 时重写 `config.toml`。`model_provider` 使用 Key 专属内部标识，`model_providers.<id>.name` 继续使用供应商名称，因此 Codex 底部不会显示内部编号。
- 是否影响接口或使用方式：无接口变化。用户切换 Key 后，Codex 侧边栏只会展示当前 Key 的历史记录；官方模式只展示官方记录。
- 是否向后兼容：已有可明确归属的导入会话会自动迁移到其 Key 专属 provider。没有归属信息且多个 Key 使用相同供应商名称的旧会话会保持归档，避免错误归属；可在 Switcher 中按正确 Key 重新导入。
- 验证方式：执行 `cargo test --lib --no-fail-fast`；新增回归测试验证两个同供应商 Key 与官方线程切换时的可见性互斥；使用当前 Codex app-server 实测 `thread/archive` 会把 rollout 移入 `archived_sessions` 并更新 SQLite 路径。
- 风险与注意事项：切换身份会自动重启已打开的 Codex。正在运行的任务应先等待完成或保存上下文；会话文件不会被删除。
