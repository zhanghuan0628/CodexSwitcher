# Third-Party Key Usage Types Change Log

- Date: 2026-04-26
- Type: backend command, credential profile schema, and frontend form enhancement

## Change

Added two optional third-party Key usage-stat types and kept them independent from the main runtime Key flow:

- `sub2api` / 语聊
- `newApi` / oneTop

If usage-stat config is missing, unsupported, or request fails, the app now hides balance/stat cards instead of throwing an error or affecting save, activate, switch, or session flows.

## Implementation Notes

- Added backend command `get_key_profile_usage(profile_id)` that returns `null` when usage stats are unavailable, instead of failing the page.
- Added backend command `update_key_profile_usage_config(...)` so usage-stat config is stored separately from the main runtime Key config.
- `sub2api` usage query:
  - uses the saved runtime API Key
  - normalizes `base_url` to `<base_url>/v1/usage`
  - sends `Authorization: Bearer <api_key>`
- `newApi` usage query:
  - uses a separate optional access token from oneTop personal security settings
  - calls `<base_url>/api/user/self`
  - sends:
    - `Authorization: Bearer <access_token>`
    - `App-Version`
    - `New-Api-User`
- oneTop access token is stored separately from the runtime Key reference, so the usage-stat credential is not mixed into Codex runtime config.
- Added optional credential-profile fields for:
  - usage provider type
  - `New-Api-User`
  - `App-Version`
  - masked usage token
  - usage token Keychain reference
- Frontend Key add/edit form now includes:
  - usage-stat type selector
  - optional oneTop access token
  - optional `New-Api-User`
  - optional `App-Version`
- Accounts page and dashboard only render Key usage blocks when a usage query returns real data.
- Key usage refresh now follows the existing frontend auto-refresh cadence when `enable_auto_refresh` is enabled and the window is visible. It does not depend on official-account sampling.
- The `立即采样` action now also refreshes Key balance/stat data after official-account sampling completes, so manual refresh behavior stays consistent.

## Impact

- Added `credential_profiles` schema columns for optional usage-stat config.
- No changes to official-account sampling logic.
- No changes to the main runtime Key activation path.
- Missing or invalid usage-stat config no longer surfaces as an inline failure card by default; the UI simply omits those metrics.

## Compatibility

- `sub2api` is compatible with YuChat-style `/v1/usage` responses.
- `newApi` is compatible with oneTop-style `/api/user/self` responses.
- `newApi` balance/stat lookup uses an access token, not the runtime API Key.

## Verification

Run:

```bash
npm run build
cargo test
npm run tauri -- build
```

Latest local result:

- `cargo test`: passed on 2026-04-26 with 65 Rust tests.
- `npm run build`: passed on 2026-04-26 after wiring usage-type selection and optional oneTop token fields.
- `npm run tauri -- build`: `.app` built successfully on 2026-04-26; DMG still fails in the existing `bundle_dmg.sh` step, which does not block the latest app bundle.
