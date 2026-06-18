# 第三方 Key 会话互导改为独立复制

- 变更背景：第三方 Key 之间导入项目会话时，旧实现复用原 Codex thread ID，并把原记录的所属 Key 改成目标 Key。界面提示导入成功，但目标 Key 没有获得独立会话，源 Key 的记录也会被迁走。
- 本次变更内容：跨第三方 Key 导入时，为“源 thread + 目标 Key”生成稳定的新 thread ID，复制 rollout 文件并同步重写 `session_meta.id` 与 `session_meta.model_provider`，再将副本写入 Switcher 项目记录库和 Codex `state_5.sqlite`。
- 重复导入：同一源会话重复导入同一个目标 Key 时复用相同副本，只更新记录，不产生重复会话。
- 涉及范围：本地 Codex 候选会话导入、项目会话归属、Codex thread 写回。
- 配置与接口：没有新增配置或前端接口；导入结果结构保持不变。
- 兼容性：已有导入记录保持兼容。新的跨 Key 导入不再修改源 Key 的原始记录和 rollout 文件。
- 验证方式：Rust 回归测试验证源、目标两条记录和两个 Codex thread 同时存在，并验证重复导入幂等。
- 注意事项：无法解析 `session_meta` 的损坏 rollout 会明确返回失败，避免生成 Codex 无法识别的半成品会话。
