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
- 候选池和项目记录库只按 Codex 主线程口径展示 `source = vscode` 的记录，过滤子任务、审批审核线程等内部记录，避免 CodexSwitcher 会话数明显大于 Codex 侧边栏。
- 已对本机既有官方账号导入记录做一次回填：只更新 `source = vscode` 且 `model_provider = custom` 的官方账号导入线程。
