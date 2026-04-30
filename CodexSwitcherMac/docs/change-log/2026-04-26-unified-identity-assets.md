# Unified Identity Assets Change Log

- Date: 2026-04-26
- Type: UI behavior and frontend view-model change

## Change

Official Codex accounts and third-party keys are presented as one switchable identity set on the dashboard and accounts page. Key identities use the existing key activation flow. Official accounts keep their existing account switching, sampling, verification, repair, default, and delete behavior.

## Implementation Notes

- Added a frontend `IdentityAsset` view model that merges official account rows and third-party key profiles without changing backend schema.
- Added a reusable unified identity table for dashboard candidates and account assets.
- Dashboard now shows “当前身份” and “推荐身份”; when the active identity is a Key it shows provider/model/masked key metadata and hides “立即采样”.
- The accounts page top list now includes both official accounts and Key identities. Key rows only expose `启用` and `编辑`; official rows keep the existing detail/default/switch/verify/repair/delete actions.
- The key create/edit form remains below the unified table, so saved Key cards are no longer duplicated in a separate list.
- The app shell subtitle and sidebar status now label the active identity as official account or Key.
- The macOS menubar/tray status now also reads the active third-party Key profile. When a Key is active, the tray shows `当前：Key · <nickname> · <model>` and disables the sampling menu item.

## Impact

- No database schema changes.
- No breaking backend command changes.
- Key rows no longer expose official-account-only actions such as sampling, repair, verification, or account details.
- Dashboard and top shell now distinguish “official account” from “key” when showing the current identity.

## Verification

Run:

```bash
npm run build
cargo test
```

Latest local result:

- `npm run build`: passed on 2026-04-26 after wiring the unified identity UI.
- `cargo test`: passed on 2026-04-26 with 54 Rust tests.
- Added Rust regression coverage for active-Key tray presentation.
