# Project Session Identity Tree Change Log

> 2026-08-28: Current-identity-left / other-identities-right behavior is preserved.
> See [the archive candidate and source-pane fix](2026-08-28-project-session-library-visibility.md).

- Date: 2026-04-26
- Type: Backend candidate source, UI behavior, and frontend view-model change

## Change

The Project Sessions page now separates the current identity's project session library from the local Codex candidate import pool. Local Codex candidates are sourced from `$HOME/.codex/state_5.sqlite` `threads`, so titles match the Codex sidebar. `model_provider = custom` threads are no longer blindly attributed to whichever Key is currently active; the app now prefers the earliest provider-specific runtime error hint found in the original rollout source file, then falls back to stored imported ownership, and only uses a generic local custom group when the source cannot be identified.

## Behavior

- The left project library shows records owned by the currently active official account or Key, plus current local Codex threads whose detected identity is the active identity.
- The right local session area shows local Codex candidates whose detected identity is not the current identity.
- The right local session area is grouped by identity, then project, then session.
- Users can select candidate sessions, whole candidate projects, or whole source identities and import them into the current identity.
- Selected imports are saved under the active identity. If the active identity is a Key, records are saved as `third_party_key / key:<id>`.
- Existing imported records are no longer reassigned just because the session list is opened.
- Official accounts, third-party Keys, and unbound local Codex imported sessions can still be represented in stored session ownership.
- Each identity can be expanded or collapsed.
- Each identity contains project groups, and each project can be expanded or collapsed.
- Each project shows its sessions by session title.
- Local Codex imports using `local_codex/local` are preserved until a user explicitly imports or reimports them into the current identity.
- Identity and project groups are collapsed by default so the page does not open into a long expanded tree.
- The left project tree and right detail list scroll independently to keep the two-column layout stable.
- Candidate import cards show the detected source identity, project, Codex sidebar title, message count, and updated time.
- Codex imported sessions now derive a fallback title from the first user message when `session_index.jsonl` only has `未命名 Codex 会话`. Existing unnamed imported records are backfilled when session records are listed, as long as their original source file is still available.
- Imported `codex_imported` records are also backfilled with the same rollout-based provider hint, so the left project library and the right import tree use the same ownership evidence.
- Key groups only show session records explicitly owned by that Key, so projects with no session records under the Key are not displayed in that Key group.
- `session_index.jsonl` remains a compatibility fallback for older local data, but `state_5.sqlite` is preferred whenever it is present.
- Internal Codex approval-review transcripts whose title starts with `The following is the Codex agent history whose request action you are assessing.` are filtered out of candidates, imports, and stored session listing.
- Mixed custom threads that touched multiple third-party Key endpoints now stay with the earliest detected provider hint from rollout history instead of jumping to the most recently active Key.

## Compatibility

- Existing database schemas are unchanged.
- Local Codex candidate responses now include `identity_key`, `identity_label`, and `identity_kind_label`.
- New backend commands expose local Codex candidates and import selected candidates into the active identity.
- Existing local project and session import records remain compatible.
- Future copy/share sync can reuse the same identity/project/session grouping.
- Existing imported local Codex records remain compatible, and when rollout evidence clearly points to a third-party Key host they are backfilled to that Key owner on read.

## Verification

Run:

```bash
npm run build
cargo test
```

Latest local result:

- `npm run build`: passed on 2026-04-26 after adding the identity/project/session tree.
- `cargo test`: passed on 2026-04-26 with 54 Rust tests.
- Latest bundle app restart: confirmed on 2026-04-26 with process path `src-tauri/target/release/bundle/macos/CodexSwitcherMac.app/Contents/MacOS/codexswitchermac`.
- `npm run build`: passed on 2026-04-26 after the collapsed-tree/style/title backfill update.
- `cargo test`: passed on 2026-04-26 with 55 Rust tests after adding the Codex fallback title regression test.
- Latest bundle app restart: confirmed on 2026-04-26 with process path `src-tauri/target/release/bundle/macos/CodexSwitcherMac.app/Contents/MacOS/codexswitchermac`.
- `npm run build`: passed on 2026-04-26 after current-identity library and selectable import pool changes.
- `cargo test import_codex`: passed on 2026-04-26 after selective local Codex import changes.
- `cargo test active_key_owner`: passed on 2026-04-26 after adding the regression test that selected local Codex candidates import into the active Key.
- Latest bundle app restart: confirmed on 2026-04-26 with process path `src-tauri/target/release/bundle/macos/CodexSwitcherMac.app/Contents/MacOS/codexswitchermac`.
- `npm run build`: passed on 2026-04-26 after switching candidates to `state_5.sqlite` and adding identity/project/session import grouping.
- `cargo test listing_codex_candidates_reads_state_threads_with_codex_titles`: passed on 2026-04-26 after adding the state thread regression test.
- `cargo test active_key_owner`: passed on 2026-04-26 after the `state_5.sqlite` candidate source change.
- `cargo test import_codex`: passed on 2026-04-26 after preserving the legacy `session_index.jsonl` fallback.
- `cargo test`: passed on 2026-04-26 with 58 Rust tests.
- Latest bundle app restart: confirmed on 2026-04-26 with process `43081` at `src-tauri/target/release/bundle/macos/CodexSwitcherMac.app/Contents/MacOS/codexswitchermac`.
- `cargo test listing_codex_candidates_excludes_internal_review_threads`: passed on 2026-04-26 after filtering internal approval-review transcripts.
- `cargo test`: passed on 2026-04-26 with 59 Rust tests after adding the internal-review-thread filter regression.
- Latest bundle app restart: confirmed on 2026-04-26 with process `46912` at `src-tauri/target/release/bundle/macos/CodexSwitcherMac.app/Contents/MacOS/codexswitchermac`.
