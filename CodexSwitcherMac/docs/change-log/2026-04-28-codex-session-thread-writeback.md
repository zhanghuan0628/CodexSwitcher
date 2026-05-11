# Codex 本地会话写回 Codex 线程索引

- 变更背景：项目会话导入后只写入 CodexSwitcher 自有 `session_records`，页面能看到，但 Codex 侧边栏不一定能看到对应会话。
- 本次变更内容：导入选中本地 Codex 会话时，在写入 CodexSwitcher 项目记录库后，同步向 `$HOME/.codex/state_5.sqlite` 的 `threads` 表注册或更新对应线程。
- 涉及范围：`import_codex_local_sessions`、`import_codex_local_session_candidates` 的返回结果新增 `codex_synced_threads` 和 `codex_skipped_threads` 统计；预览模式同步返回同名字段。
- 是否影响配置：不影响。
- 是否影响接口或使用方式：影响 Tauri 命令返回结构，新增字段向后兼容。
- 是否向后兼容：兼容。没有 `state_5.sqlite` 或原始 rollout 文件不存在时，会跳过 Codex 写回，不影响 CodexSwitcher 内部导入结果。
- 验证方式：`cargo test`、`npm run build`，并新增回归测试验证导入后 thread 写入 Codex state。
- 风险与注意事项：写回只注册已有 rollout 文件，不伪造完整会话内容；Codex 若调整 `threads` 表结构，仍需要重新适配。

## 2026-04-28 补充

- 官方账号导入的本地主线程写回 Codex 时，`model_provider` 统一写为 `openai`，避免从第三方 Key 导入到官方账号后仍被 Codex 官方账号视图过滤。
- 官方账号写回时同步更新 rollout 首行 `session_meta.payload.model_provider`，避免 Codex 重建索引后又把已导入会话恢复成 `custom`。
- Key 导入的本地主线程写回 Codex 时，`model_provider` 统一写为 `custom`，避免从官方账号导入到当前 Key 后仍被 Codex Key 视图过滤。
- Key 写回时同步更新 rollout 首行 `session_meta.payload.model_provider`，避免 Codex 重建索引后又把已导入会话恢复成 `openai`。
- 项目记录库读取时会轻量修正既有导入记录：若记录已归属 Key，但 Codex `threads` 仍是 `openai`，会回填为 `custom` 并解除归档；若记录归属官方账号，则回填为 `openai`。
- 候选池和项目记录库只按 Codex 主线程口径展示 `source = vscode` 的记录，过滤子任务、审批审核线程等内部记录，避免 CodexSwitcher 会话数明显大于 Codex 侧边栏。
- 已对本机既有官方账号导入记录做一次回填：只更新 `source = vscode` 且 `model_provider = custom` 的官方账号导入线程。

## 2026-05-11 补充

- 第三方 Key 写回 Codex 线程索引时，不再固定使用 `custom` 作为 `model_provider`。
- 优先读取当前 Codex `config.toml` 顶层 `model_provider`；读取不到时使用 CodexSwitcher 中当前 Key 的供应商字段，最后才兼容兜底为 `custom`。
- 第三方 Key 运行配置生成时，`model_provider` 和 `[model_providers]` 表名改为使用 Key 的供应商字段，支持用户改名或不同安装环境使用非 `custom` 名称。
- 候选会话归类不再只把 `custom` 识别为 Key，会把非 `openai` 的 `model_provider` 作为第三方 Key 来源展示。
- 验证方式：`cargo test`。
