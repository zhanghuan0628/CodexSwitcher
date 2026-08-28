import type { CodexLocalSessionCandidate, LocalProject, SessionRecord } from "../types";
import type { IdentityAsset } from "./identityViewModel";

export type ProjectSessionSelection =
  | { kind: "all" }
  | { kind: "identity"; identityKey: string }
  | { kind: "project"; identityKey: string; projectId: number }
  | { kind: "session"; identityKey: string; projectId: number; sessionId: number };

export type ProjectSessionIdentityGroup = {
  key: string;
  label: string;
  subtitle: string;
  kindLabel: string;
  sessionCount: number;
  projectCount: number;
  projects: ProjectSessionProjectGroup[];
};

export type ProjectSessionProjectGroup = {
  id: number;
  name: string;
  path: string;
  updatedAt: string;
  sessions: SessionRecord[];
};

export type ProjectSessionScope = {
  identity: ProjectSessionIdentityGroup;
  project: ProjectSessionProjectGroup;
};

const UNNAMED_CODEX_SESSION_TITLES = new Set(["", "未命名会话", "未命名 Codex 会话"]);

function fallbackProject(record: SessionRecord): LocalProject {
  return {
    id: record.project_id,
    name: record.project_name,
    workspace_path: record.project_path,
    git_remote: null,
    last_active_at: record.updated_at,
    created_at: record.created_at,
    updated_at: record.updated_at,
  };
}

function isUnnamedCodexSessionTitle(title: string) {
  return UNNAMED_CODEX_SESSION_TITLES.has(title.trim());
}

export function displaySessionTitle(record: SessionRecord) {
  if (!isUnnamedCodexSessionTitle(record.title)) {
    return record.title.trim();
  }

  const projectName = record.project_name?.trim() || "Codex 会话";
  const updatedAt = record.updated_at?.trim();
  if (updatedAt) {
    return `${projectName} · ${updatedAt}`;
  }
  if (record.message_count > 0) {
    return `${projectName} · ${record.message_count} 条消息`;
  }
  return `${projectName} · Codex 会话`;
}

export function sessionIdentityKey(record: SessionRecord) {
  if (record.owner_profile_kind === "official_account" && record.owner_account_id) {
    return `account:${record.owner_account_id}`;
  }
  if (record.owner_profile_kind === "third_party_key") {
    return record.owner_profile_ref.startsWith("key:")
      ? record.owner_profile_ref
      : `key:${record.owner_profile_ref}`;
  }
  return `${record.owner_profile_kind}:${record.owner_profile_ref}`;
}

function identityLabel(key: string, assets: IdentityAsset[]) {
  const asset = assets.find((item) => item.id === key);
  if (asset?.kind === "official_account") {
    return {
      label: asset.title,
      subtitle: asset.subtitle,
      kindLabel: "官方账号",
    };
  }
  if (asset?.kind === "third_party_key") {
    return {
      label: asset.title,
      subtitle: [asset.profile.provider, asset.profile.model].filter(Boolean).join(" · ") || "第三方 Key",
      kindLabel: "Key",
    };
  }
  if (key === "local_codex:local") {
    return {
      label: "本地 Codex 导入",
      subtitle: "未绑定账号/Key",
      kindLabel: "未绑定",
    };
  }
  if (key.startsWith("account:") || key.startsWith("key:")) {
    const kindLabel = key.startsWith("account:") ? "官方账号" : "Key";
    return {
      label: `历史${kindLabel}（ID ${key.split(":")[1]}）`,
      subtitle: "原身份已移除，名称不可用",
      kindLabel,
    };
  }
  return {
    label: key,
    subtitle: "未知身份来源",
    kindLabel: "其他",
  };
}

export function buildProjectSessionGroups({
  identityAssets,
  localProjects,
  sessionRecords,
}: {
  identityAssets: IdentityAsset[];
  localProjects: LocalProject[];
  sessionRecords: SessionRecord[];
}): ProjectSessionIdentityGroup[] {
  const projectById = new Map(localProjects.map((project) => [project.id, project]));
  const grouped = new Map<string, Map<number, SessionRecord[]>>();

  for (const record of sessionRecords) {
    const identityKey = sessionIdentityKey(record);
    const projects = grouped.get(identityKey) ?? new Map<number, SessionRecord[]>();
    const sessions = projects.get(record.project_id) ?? [];
    sessions.push(record);
    projects.set(record.project_id, sessions);
    grouped.set(identityKey, projects);
  }

  return [...grouped.entries()]
    .map(([identityKey, projects]) => {
      const label = identityLabel(identityKey, identityAssets);
      const projectGroups = [...projects.entries()]
        .map(([projectId, sessions]) => {
          const project = projectById.get(projectId) ?? fallbackProject(sessions[0]);
          const sortedSessions = [...sessions].sort((left, right) => right.updated_at.localeCompare(left.updated_at));
          return {
            id: project.id,
            name: project.name,
            path: project.workspace_path,
            updatedAt: project.last_active_at ?? project.updated_at,
            sessions: sortedSessions,
          };
        })
        .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));

      return {
        key: identityKey,
        label: label.label,
        subtitle: label.subtitle,
        kindLabel: label.kindLabel,
        sessionCount: projectGroups.reduce((total, project) => total + project.sessions.length, 0),
        projectCount: projectGroups.length,
        projects: projectGroups,
      };
    })
    .sort((left, right) => {
      if (left.key === "local_codex:local") return 1;
      if (right.key === "local_codex:local") return -1;
      return right.sessionCount - left.sessionCount || left.label.localeCompare(right.label, "zh-Hans-CN");
    });
}

export function sessionsForProjectSelection(
  groups: ProjectSessionIdentityGroup[],
  selection: ProjectSessionSelection,
) {
  if (selection.kind === "all") {
    return groups.flatMap((identity) => identity.projects.flatMap((project) => project.sessions));
  }

  const identity = groups.find((item) => item.key === selection.identityKey);
  if (!identity) return [];

  if (selection.kind === "identity") {
    return identity.projects.flatMap((project) => project.sessions);
  }

  const project = identity.projects.find((item) => item.id === selection.projectId);
  if (!project) return [];

  if (selection.kind === "project") {
    return project.sessions;
  }

  return project.sessions.filter((session) => session.id === selection.sessionId);
}

export function buildProjectSessionScopeBySessionId(groups: ProjectSessionIdentityGroup[]) {
  const lookup = new Map<number, ProjectSessionScope>();
  for (const identity of groups) {
    for (const project of identity.projects) {
      for (const session of project.sessions) {
        lookup.set(session.id, { identity, project });
      }
    }
  }
  return lookup;
}

export function selectedProjectSessionTitle(selection: ProjectSessionSelection, groups: ProjectSessionIdentityGroup[]) {
  if (selection.kind === "all") return "全部项目会话";
  const identity = groups.find((item) => item.key === selection.identityKey);
  if (!identity) return "项目会话";
  if (selection.kind === "identity") return identity.label;
  const project = identity.projects.find((item) => item.id === selection.projectId);
  if (!project) return identity.label;
  if (selection.kind === "project") return project.name;
  const session = project.sessions.find((item) => item.id === selection.sessionId);
  return session ? displaySessionTitle(session) : "项目会话";
}

function stableNegativeId(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) | 0;
  }
  return -Math.abs(hash || 1);
}

function sessionExternalId(record: SessionRecord) {
  try {
    const content = JSON.parse(record.raw_content) as { session_id?: unknown };
    return typeof content.session_id === "string" ? content.session_id : null;
  } catch {
    return null;
  }
}

// Only the active identity belongs in the destination library.
export function buildProjectSessionRecords({
  activeIdentity,
  localProjects,
  sessionRecords,
  candidates,
}: {
  activeIdentity: IdentityAsset | null;
  localProjects: LocalProject[];
  sessionRecords: SessionRecord[];
  candidates: CodexLocalSessionCandidate[];
}): SessionRecord[] {
  if (!activeIdentity) return [];
  const records = sessionRecords.filter((record) => sessionIdentityKey(record) === activeIdentity.id);
  const existingExternalIds = new Set(sessionRecords.map(sessionExternalId).filter(Boolean));
  const projectIdByPath = new Map(localProjects.map((project) => [project.workspace_path, project.id]));
  for (const candidate of candidates) {
    if (candidate.identity_key !== activeIdentity.id || existingExternalIds.has(candidate.candidate_id)) continue;
    const identity = activeIdentity;
    existingExternalIds.add(candidate.candidate_id);
    records.push({
      id: stableNegativeId(`session:${candidate.candidate_id}`),
      project_id: projectIdByPath.get(candidate.project_path) ?? stableNegativeId(`project:${candidate.project_path}`),
      project_name: candidate.project_name,
      project_path: candidate.project_path,
      owner_account_id: identity.account?.id ?? null,
      owner_profile_kind: identity.kind,
      owner_profile_ref: identity.id,
      record_type: "codex_local_thread",
      title: candidate.title,
      summary: `Codex 本地会话 · ${candidate.message_count} 条消息 · ${candidate.project_path}`,
      raw_content: JSON.stringify({ source: "codex_state_thread", session_id: candidate.candidate_id, source_path: candidate.source_path }),
      message_count: candidate.message_count,
      source_record_id: null,
      created_at: candidate.created_at,
      updated_at: candidate.updated_at,
    });
  }
  return records;
}

export type ProjectSessionImportCandidate = CodexLocalSessionCandidate & { importable: boolean };

// Preserve other identities' history in the source pane, but only enable real import sources.
export function buildProjectSessionImportCandidates({
  activeIdentityKey,
  identityAssets,
  sessionRecords,
  candidates,
}: {
  activeIdentityKey: string | null;
  identityAssets: IdentityAsset[];
  sessionRecords: SessionRecord[];
  candidates: CodexLocalSessionCandidate[];
}): ProjectSessionImportCandidate[] {
  const currentIds = new Set(sessionRecords
    .filter((record) => sessionIdentityKey(record) === activeIdentityKey)
    .map(sessionExternalId).filter(Boolean));
  const byId = new Map<string, ProjectSessionImportCandidate>();
  const sourceById = new Map(candidates.map((candidate) => [candidate.candidate_id, candidate]));
  for (const record of sessionRecords) {
    const key = sessionIdentityKey(record);
    const externalId = sessionExternalId(record);
    if (key === activeIdentityKey || (externalId && currentIds.has(externalId))) continue;
    const source = externalId ? sourceById.get(externalId) : undefined;
    const label = identityLabel(key, identityAssets);
    const id = externalId ?? `record:${record.id}`;
    if (byId.has(id)) continue;
    byId.set(id, {
      candidate_id: id,
      identity_key: key,
      identity_label: label.label,
      identity_kind_label: label.kindLabel,
      project_name: record.project_name,
      project_path: record.project_path,
      title: displaySessionTitle(record),
      message_count: source?.message_count ?? record.message_count,
      source_path: source?.source_path ?? "",
      created_at: record.created_at,
      updated_at: source?.updated_at ?? record.updated_at,
      imported_session_id: record.id,
      imported_owner_profile_kind: record.owner_profile_kind,
      imported_owner_profile_ref: record.owner_profile_ref,
      importable: Boolean(source),
    });
  }
  for (const candidate of candidates) {
    if (candidate.identity_key === activeIdentityKey || currentIds.has(candidate.candidate_id)
      || byId.has(candidate.candidate_id)) continue;
    byId.set(candidate.candidate_id, { ...candidate, importable: true });
  }
  return [...byId.values()].sort((left, right) => right.updated_at.localeCompare(left.updated_at));
}
