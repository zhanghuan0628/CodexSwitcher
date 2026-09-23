# 严格按 Codex provider 隔离会话

- 变更背景：按账号切换可见会话时，项目库中的已导入记录 ID 曾能覆盖 Codex thread 自身的 `model_provider`，导致属于第三方 Key 的会话在官方账号下被解除归档；provider 回填也可能把已有 Key 专属 provider 改成另一身份的 provider。
- 本次变更内容：侧栏可见性只接受与当前身份精确匹配的 provider，第三方 Key 仍兼容唯一匹配的旧版供应商名；已明确绑定到另一个 Key 的 `codexswitcher-key-<id>` 不再因项目库记录而改写，官方账号也不会因此显示或接管该 Key 会话。
- 涉及范围：Codex thread provider 回填、账号切换后的会话归档与可见性同步。
- 是否影响配置：否。
- 是否影响接口或使用方式：无接口变化；官方身份仅显示 `openai` 会话，第三方 Key 仅显示其专属 provider 会话。
- 是否向后兼容：兼容旧的未限定供应商 provider；已有 Key 专属 provider 冲突会保持原归属并按非当前身份归档。
- 验证方式：检查 provider 回填与可见性同步的代码路径及差异；本次未运行测试。
- 注意事项：原有跨身份导入副本通过独立 thread ID 和目标 Key provider 维持可见，不需要复用源会话 ID。
