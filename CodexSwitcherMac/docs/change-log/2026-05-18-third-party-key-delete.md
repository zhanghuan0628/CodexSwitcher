# 第三方 Key 删除能力

- 变更背景：第三方 Key 保存后只能启用或编辑，缺少删除入口。
- 本次变更内容：统一身份资产列表为非当前第三方 Key 增加删除按钮，第一次点击会在应用内切换为“确认删除”，第二次点击才会删除，并移除本机保存的 Keychain 凭证和对应的 Key 资产记录。
- 涉及范围：Mac 端账号中心、前端预览 API、Tauri API、SQLite `credential_profiles` 删除逻辑。
- 是否影响配置：不新增配置项。
- 是否影响接口或使用方式：新增 Tauri 命令 `delete_credential_profile(profile_id)`；前端 API 增加 `deleteCredentialProfile(profileId)`。
- 是否向后兼容：兼容。官方账号资产不能通过该命令删除；当前登录的 Key 不能删除。
- 额外保护：当前 active 官方账号，以及当前 Codex 登录态匹配的官方账号，都会拒绝删除。
- 验证方式：新增 Rust 单元测试覆盖非当前 Key 可删除、当前 Key 不可删除、当前官方账号不可删除。
- 风险与注意事项：删除 Key 不会删除历史会话记录；历史记录仍保留原 `key:<id>` 引用作为审计痕迹。
- 交互修正：删除确认不依赖系统 `window.confirm`，避免打包后的 WebView 点击后无可见反馈。
