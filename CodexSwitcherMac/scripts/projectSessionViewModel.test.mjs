import assert from "node:assert/strict";
import test from "node:test";
import { build } from "esbuild";

const result = await build({
  entryPoints: [new URL("../src/pages/projectSessionViewModel.ts", import.meta.url).pathname],
  bundle: true,
  format: "esm",
  write: false,
});
const { buildProjectSessionRecords, buildProjectSessionImportCandidates, buildProjectSessionGroups, sessionsForProjectSelection } =
  await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString("base64")}`);

const identityAssets = [
  { id: "account:1", kind: "official_account", title: "官方账号", account: { id: 1 }, profile: {} },
  { id: "key:9", kind: "third_party_key", title: "Key A", account: null, profile: { provider: "same-host" } },
  { id: "key:10", kind: "third_party_key", title: "Key B", account: null, profile: { provider: "same-host" } },
];
const candidate = (candidate_id, identity_key) => ({
  candidate_id, identity_key, project_name: "Project", project_path: "/tmp/project",
  title: candidate_id, message_count: 2, source_path: `/tmp/${candidate_id}.jsonl`,
  created_at: "2026-08-28 10:00:00", updated_at: "2026-08-28 11:00:00",
});
const stored = {
  id: 1, project_id: 1, project_name: "Project", project_path: "/tmp/project",
  owner_account_id: null, owner_profile_kind: "third_party_key", owner_profile_ref: "key:9",
  record_type: "codex_imported", title: "Stored history", raw_content: '{"session_id":"stored"}',
  message_count: 1, created_at: "2026-08-27 10:00:00", updated_at: "2026-08-27 11:00:00",
};

test("left pane contains only the current account or Key, right pane contains other identities", () => {
  const candidates = [candidate("official", "account:1"), candidate("key-b", "key:10")];
  for (const activeIdentity of identityAssets) {
    const records = buildProjectSessionRecords({
      activeIdentity, localProjects: [], sessionRecords: [stored], candidates,
    });
    const groups = buildProjectSessionGroups({ identityAssets, localProjects: [], sessionRecords: records });
    assert.deepEqual(groups.map((group) => group.key), [activeIdentity.id]);
    assert.equal(groups[0].label, activeIdentity.title);
    assert.equal(sessionsForProjectSelection(groups, { kind: "all" }).length, 1);
    const sources = buildProjectSessionImportCandidates({
      activeIdentityKey: activeIdentity.id, identityAssets, sessionRecords: [stored], candidates,
    });
    assert.equal(sources.length, 2);
    assert.ok(sources.every((item) => item.identity_key !== activeIdentity.id));
  }
});

test("no active identity leaves the destination empty", () => {
  assert.deepEqual(buildProjectSessionRecords({ activeIdentity: null, localProjects: [],
    sessionRecords: [stored], candidates: [candidate("official", "account:1")] }), []);
});

test("persisted ownership wins over duplicate or inconsistently attributed local candidates", () => {
  const candidates = [candidate("stored", "account:1"), candidate("new", "key:9"),
    candidate("new", "key:9"), candidate("unknown", "codex_provider:custom")];
  const records = buildProjectSessionRecords({
    activeIdentity: identityAssets[1], localProjects: [], sessionRecords: [stored], candidates,
  });
  assert.equal(records.length, 2);
  assert.equal(records[0], stored);
  assert.equal(records[1].owner_profile_ref, "key:9");
  const sources = buildProjectSessionImportCandidates({
    activeIdentityKey: "key:9", identityAssets, sessionRecords: [stored], candidates,
  });
  assert.deepEqual(sources.map((item) => item.candidate_id), ["unknown"]);
});

test("other identity history without a source stays on the right but cannot be imported", () => {
  const sources = buildProjectSessionImportCandidates({
    activeIdentityKey: "account:1", identityAssets: [], sessionRecords: [stored], candidates: [],
  });
  assert.equal(sources.length, 1);
  assert.equal(sources[0].identity_label, "历史Key（ID 9）");
  assert.equal(sources[0].importable, false);
});

test("imported records appear only on the destination side and source identity names remain intact", () => {
  const candidates = [candidate("stored", "key:9")];
  const before = buildProjectSessionImportCandidates({
    activeIdentityKey: "account:1", identityAssets, sessionRecords: [stored], candidates,
  });
  assert.equal(before[0].importable, true);
  assert.equal(before[0].identity_label, "Key A");
  const imported = { ...stored, owner_account_id: 1, owner_profile_kind: "official_account", owner_profile_ref: "account:1" };
  const records = buildProjectSessionRecords({ activeIdentity: identityAssets[0], localProjects: [],
    sessionRecords: [imported], candidates });
  assert.equal(records.length, 1);
  assert.equal(records[0], imported);
  assert.deepEqual(buildProjectSessionImportCandidates({
    activeIdentityKey: "account:1", identityAssets, sessionRecords: [imported], candidates,
  }), []);
});
