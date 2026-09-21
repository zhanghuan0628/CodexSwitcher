# 跨 Key 导入会话恢复兼容修复

- 变更背景：从其他第三方 Key 导入会话后，Codex 桌面端打开副本时提示 `list_turns is not supported yet`，无法恢复对话。
- 本次变更内容：导入线程写回 `state_5.sqlite` 以及既有导入记录回填时，如果 `threads.history_mode` 字段存在，统一设置为 `paginated`，匹配新版 Codex 的 `thread/resume` 历史加载方式。
- 涉及范围：跨第三方 Key 会话导入、Codex thread 索引元数据回填。
- 是否影响配置：否。
- 是否影响接口或使用方式：否，前端导入接口和结果结构不变。
- 是否向后兼容：兼容。旧版 Codex state 没有 `history_mode` 字段时跳过更新；已有错误导入记录会在下次会话记录刷新或重新导入时自动修复。
- 验证方式：Rust 导入回归测试验证写回后的 `history_mode = 'paginated'`，并通过完整 Rust 测试与前端构建。
- 注意事项：修复仅针对 Switcher 写入的线程元数据，不修改 Codex 原始 rollout 内容以外的历史消息。
