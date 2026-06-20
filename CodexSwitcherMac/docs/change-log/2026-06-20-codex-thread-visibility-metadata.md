# Codex 导入线程补齐新版可见性元数据

- 变更背景：跨 Key 导入项目会话时，Switcher 已写入目标 Key 的项目记录库和 Codex `state_5.sqlite`，但 Codex 新版侧边栏会依赖 `threads.preview`、`thread_source`、`recency_at(_ms)` 等可见性字段。旧写回只写基础 thread 字段，导致界面提示导入成功，但 Codex 侧边栏不展示导入副本。
- 本次变更内容：导入写回 Codex thread 后，如果检测到新版字段存在，会补写 `thread_source = user`、非空 `preview` 以及最近时间字段；已有导入记录的自愈逻辑也会补齐这些字段。
- 涉及范围：`import_codex_local_session_candidates`、项目会话列表刷新时的 Codex state 自愈、跨第三方 Key 会话复制。
- 是否影响配置：否。
- 是否影响接口或使用方式：否，前端接口和导入结果结构不变。
- 是否向后兼容：兼容。字段存在时才写入，旧版 Codex `threads` 表不会报错。
- 验证方式：已执行 Rust 回归测试，覆盖导入写回、官方会话导入到 Key、跨 Key 复制和既有导入记录自愈。
- 注意事项：本机已备份并修复 `~/.codex/state_5.sqlite` 中 taomu 目标 Key 的已导入线程 `1677cce3-114e-59a7-8685-33f3a21a8b9c`；如果 Codex 界面仍未刷新，需要重启或刷新 Codex 侧边栏进程。
