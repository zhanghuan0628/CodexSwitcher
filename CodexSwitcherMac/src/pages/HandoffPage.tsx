import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { SectionHeader } from "../components/SectionHeader";
import type { CodexLocalSessionCandidate, LocalProject, SessionRecord } from "../types";
import type { IdentityAsset } from "./identityViewModel";
import {
  buildProjectSessionGroups,
  displaySessionTitle,
  selectedProjectSessionTitle,
  sessionIdentityKey,
  sessionsForProjectSelection,
} from "./projectSessionViewModel";
import type { ProjectSessionSelection } from "./projectSessionViewModel";

type ProjectSessionsPageProps = {
  identityAssets: IdentityAsset[];
  activeIdentity: IdentityAsset | null;
  localProjects: LocalProject[];
  sessionRecords: SessionRecord[];
  codexLocalSessionCandidates: CodexLocalSessionCandidate[];
  importingCodexSessions: boolean;
  onImportCodexLocalSessions: (candidateIds?: string[]) => void | Promise<void>;
};

function projectTreeKey(identityKey: string, projectId: number) {
  return `${identityKey}::${projectId}`;
}

function selectionMatches(
  selection: ProjectSessionSelection,
  kind: ProjectSessionSelection["kind"],
  identityKey?: string,
  projectId?: number,
  sessionId?: number,
) {
  if (selection.kind !== kind) return false;
  if ("identityKey" in selection && selection.identityKey !== identityKey) return false;
  if ("projectId" in selection && selection.projectId !== projectId) return false;
  if ("sessionId" in selection && selection.sessionId !== sessionId) return false;
  return true;
}

function sessionTitle(record: SessionRecord) {
  return displaySessionTitle(record);
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

function candidateAsSessionRecord(
  candidate: CodexLocalSessionCandidate,
  activeIdentity: IdentityAsset,
  projectId: number,
): SessionRecord {
  return {
    id: stableNegativeId(`session:${candidate.candidate_id}`),
    project_id: projectId,
    project_name: candidate.project_name,
    project_path: candidate.project_path,
    owner_account_id: activeIdentity.account?.id ?? null,
    owner_profile_kind: activeIdentity.kind,
    owner_profile_ref: activeIdentity.id,
    record_type: "codex_local_thread",
    title: candidate.title,
    summary: `Codex 本地会话 · ${candidate.message_count} 条消息 · ${candidate.project_path}`,
    raw_content: JSON.stringify({ source: "codex_state_thread", session_id: candidate.candidate_id, source_path: candidate.source_path }),
    message_count: candidate.message_count,
    source_record_id: null,
    created_at: candidate.created_at,
    updated_at: candidate.updated_at,
  };
}

export function HandoffPage({
  identityAssets,
  activeIdentity,
  localProjects,
  sessionRecords,
  codexLocalSessionCandidates,
  importingCodexSessions,
  onImportCodexLocalSessions,
}: ProjectSessionsPageProps) {
  const activeIdentityKey = activeIdentity?.id ?? null;
  const uniqueCodexLocalSessionCandidates = useMemo(() => {
    const byId = new Map<string, CodexLocalSessionCandidate>();
    for (const candidate of codexLocalSessionCandidates) {
      const existing = byId.get(candidate.candidate_id);
      if (!existing || candidate.updated_at.localeCompare(existing.updated_at) > 0) {
        byId.set(candidate.candidate_id, candidate);
      }
    }
    return [...byId.values()].sort((left, right) => right.updated_at.localeCompare(left.updated_at));
  }, [codexLocalSessionCandidates]);
  const currentIdentityRecords = useMemo(
    () => {
      if (!activeIdentityKey || !activeIdentity) return [];
      const records = sessionRecords.filter((record) => sessionIdentityKey(record) === activeIdentityKey);
      const existingExternalIds = new Set(records.map(sessionExternalId).filter(Boolean));
      const projectIdByPath = new Map(localProjects.map((project) => [project.workspace_path, project.id]));
      const currentLocalThreads = uniqueCodexLocalSessionCandidates
        .filter((candidate) => candidate.identity_key === activeIdentityKey && !existingExternalIds.has(candidate.candidate_id))
        .map((candidate) => candidateAsSessionRecord(
          candidate,
          activeIdentity,
          projectIdByPath.get(candidate.project_path) ?? stableNegativeId(`project:${candidate.project_path}`),
        ));
      return [...records, ...currentLocalThreads];
    },
    [activeIdentity, activeIdentityKey, localProjects, sessionRecords, uniqueCodexLocalSessionCandidates],
  );
  const currentIdentityProjects = useMemo(() => {
    const byPath = new Map(localProjects.map((project) => [project.workspace_path, project]));
    const virtualProjects = currentIdentityRecords
      .filter((record) => !byPath.has(record.project_path))
      .map((record) => ({
        id: record.project_id,
        name: record.project_name,
        workspace_path: record.project_path,
        git_remote: null,
        last_active_at: record.updated_at,
        created_at: record.created_at,
        updated_at: record.updated_at,
      }));
    return [...localProjects, ...virtualProjects];
  }, [currentIdentityRecords, localProjects]);
  const groups = useMemo(
    () => buildProjectSessionGroups({ identityAssets, localProjects: currentIdentityProjects, sessionRecords: currentIdentityRecords }),
    [currentIdentityProjects, currentIdentityRecords, identityAssets],
  );
  const [selection, setSelection] = useState<ProjectSessionSelection>({ kind: "all" });
  const [expandedIdentities, setExpandedIdentities] = useState<Set<string>>(() => new Set());
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(() => new Set());
  const [expandedImportIdentities, setExpandedImportIdentities] = useState<Set<string>>(() => new Set());
  const [expandedImportProjects, setExpandedImportProjects] = useState<Set<string>>(() => new Set());
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    const identityKeys = new Set(groups.map((group) => group.key));
    const projectKeys = new Set(
      groups.flatMap((group) => group.projects.map((project) => projectTreeKey(group.key, project.id))),
    );
    setExpandedIdentities((current) => {
      return new Set([...current].filter((key) => identityKeys.has(key)));
    });
    setExpandedProjects((current) => {
      return new Set([...current].filter((key) => projectKeys.has(key)));
    });
  }, [groups]);

  const projectCount = groups.reduce((total, group) => total + group.projectCount, 0);
  const sessionCount = currentIdentityRecords.length;
  const projectCountText = `${projectCount}`;
  const sessionCountText = `${sessionCount}`;
  const selectedSessions = useMemo(
    () => sessionsForProjectSelection(groups, selection),
    [groups, selection],
  );
  const selectedTitle = selectedProjectSessionTitle(selection, groups);
  const importCandidates = useMemo(
    () => uniqueCodexLocalSessionCandidates.filter((candidate) => candidate.identity_key !== activeIdentityKey),
    [activeIdentityKey, uniqueCodexLocalSessionCandidates],
  );
  const importCandidateIdentities = useMemo(() => {
    const grouped = new Map<string, CodexLocalSessionCandidate[]>();
    for (const candidate of importCandidates) {
      const sessions = grouped.get(candidate.identity_key) ?? [];
      sessions.push(candidate);
      grouped.set(candidate.identity_key, sessions);
    }
    return [...grouped.entries()]
      .map(([identityKey, sessions]) => {
        const projectGroups = new Map<string, CodexLocalSessionCandidate[]>();
        for (const session of sessions) {
          const projectSessions = projectGroups.get(session.project_path) ?? [];
          projectSessions.push(session);
          projectGroups.set(session.project_path, projectSessions);
        }
        const projects = [...projectGroups.entries()]
          .map(([projectPath, projectSessions]) => ({
            key: `${identityKey}::${projectPath}`,
            projectPath,
            projectName: projectSessions[0]?.project_name ?? projectPath,
            sessions: [...projectSessions].sort((left, right) => right.updated_at.localeCompare(left.updated_at)),
          }))
          .sort((left, right) => right.sessions[0].updated_at.localeCompare(left.sessions[0].updated_at));
        return {
          identityKey,
          identityLabel: sessions[0]?.identity_label ?? identityKey,
          identityKindLabel: sessions[0]?.identity_kind_label ?? "其他",
          sessionCount: sessions.length,
          projectCount: projects.length,
          updatedAt: projects[0]?.sessions[0]?.updated_at ?? "",
          projects,
        };
      })
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
  }, [importCandidates]);
  useEffect(() => {
    const identityKeys = new Set(importCandidateIdentities.map((group) => group.identityKey));
    const projectKeys = new Set(
      importCandidateIdentities.flatMap((group) => group.projects.map((project) => project.key)),
    );
    const candidateIds = new Set(importCandidates.map((candidate) => candidate.candidate_id));
    setExpandedImportIdentities((current) => new Set([...current].filter((key) => identityKeys.has(key))));
    setExpandedImportProjects((current) => new Set([...current].filter((key) => projectKeys.has(key))));
    setSelectedCandidateIds((current) => new Set([...current].filter((key) => candidateIds.has(key))));
  }, [importCandidateIdentities, importCandidates]);
  const selectedCandidateCount = selectedCandidateIds.size;
  const activeIdentityLabel = activeIdentity
    ? `${activeIdentity.kind === "third_party_key" ? "Key" : "官方账号"} · ${activeIdentity.title}`
    : "未设置当前身份";

  function toggleCandidate(candidateId: string) {
    setSelectedCandidateIds((current) => {
      const next = new Set(current);
      if (next.has(candidateId)) next.delete(candidateId);
      else next.add(candidateId);
      return next;
    });
  }

  function toggleProjectCandidates(candidates: CodexLocalSessionCandidate[]) {
    setSelectedCandidateIds((current) => {
      const next = new Set(current);
      const allSelected = candidates.every((candidate) => next.has(candidate.candidate_id));
      for (const candidate of candidates) {
        if (allSelected) next.delete(candidate.candidate_id);
        else next.add(candidate.candidate_id);
      }
      return next;
    });
  }

  function toggleImportIdentityCandidates(candidates: CodexLocalSessionCandidate[]) {
    toggleProjectCandidates(candidates);
  }

  function handleCheckboxClick(event: MouseEvent<HTMLInputElement>) {
    event.stopPropagation();
  }

  async function importSelectedCandidates() {
    const ids = [...selectedCandidateIds];
    if (!ids.length) return;
    await onImportCodexLocalSessions(ids);
    setSelectedCandidateIds(new Set());
  }

  return (
    <section className="handoff-workspace project-session-workspace">
      <article className="workspace-card handoff-workspace__editor project-session-tree-card">
        <SectionHeader
          eyebrow="本地项目"
          title="项目记录库"
          description={`当前身份：${activeIdentityLabel}。这里只显示当前登录账号或 Key 已导入的项目会话。`}
        />
        <div className="metric-grid compact-metric-grid project-session-metrics">
          <div className="metric-tile">
            <span className="metric-tile__label">项目</span>
            <strong className="metric-tile__value">{projectCountText}</strong>
          </div>
          <div className="metric-tile">
            <span className="metric-tile__label">会话</span>
            <strong className="metric-tile__value">{sessionCountText}</strong>
          </div>
        </div>
        <div className="project-session-tree">
          <button
            className={`project-session-tree__all ${selection.kind === "all" ? "is-active" : ""}`}
            type="button"
            onClick={() => setSelection({ kind: "all" })}
          >
            <span>全部身份</span>
            <strong>{groups.length} 类身份 · {sessionCount} 个会话</strong>
          </button>
          {groups.length ? groups.map((identity) => {
            const identityOpen = expandedIdentities.has(identity.key);
            return (
              <div className="project-session-identity" key={identity.key}>
                <div className="project-session-identity__row">
                  <button
                    className="project-session-toggle"
                    type="button"
                    aria-expanded={identityOpen}
                    onClick={() => setExpandedIdentities((current) => {
                      const next = new Set(current);
                      if (next.has(identity.key)) next.delete(identity.key);
                      else next.add(identity.key);
                      return next;
                    })}
                  >
                    <span aria-hidden="true">{identityOpen ? "▾" : "▸"}</span>
                  </button>
                  <button
                    className={`project-session-node project-session-node--identity ${selectionMatches(selection, "identity", identity.key) ? "is-active" : ""}`}
                    type="button"
                    onClick={() => setSelection({ kind: "identity", identityKey: identity.key })}
                  >
                    <span className="project-session-node__main">
                      <span className="project-session-node__title">{identity.label}</span>
                      <span className="project-session-node__badge">{identity.kindLabel}</span>
                    </span>
                    <span className="project-session-node__meta">
                      {identity.subtitle} · {identity.projectCount} 项目 · {identity.sessionCount} 会话
                    </span>
                  </button>
                </div>
                {identityOpen ? (
                  <div className="project-session-project-list">
                    {identity.projects.map((project) => {
                      const projectKey = projectTreeKey(identity.key, project.id);
                      const projectOpen = expandedProjects.has(projectKey);
                      return (
                        <div className="project-session-project" key={projectKey}>
                          <div className="project-session-project__row">
                            <button
                              className="project-session-toggle"
                              type="button"
                              aria-expanded={projectOpen}
                              onClick={() => setExpandedProjects((current) => {
                                const next = new Set(current);
                                if (next.has(projectKey)) next.delete(projectKey);
                                else next.add(projectKey);
                                return next;
                              })}
                            >
                              <span aria-hidden="true">{projectOpen ? "▾" : "▸"}</span>
                            </button>
                            <button
                              className={`project-session-node project-session-node--project ${selectionMatches(selection, "project", identity.key, project.id) ? "is-active" : ""}`}
                              type="button"
                              onClick={() => setSelection({ kind: "project", identityKey: identity.key, projectId: project.id })}
                            >
                              <span className="project-session-node__main">
                                <span className="project-session-node__title">{project.name}</span>
                                <span className="project-session-node__badge">{project.sessions.length}</span>
                              </span>
                              <span className="project-session-node__meta">{project.sessions.length} 会话 · {project.path}</span>
                            </button>
                          </div>
                          {projectOpen ? (
                            <div className="project-session-session-list">
                              {project.sessions.map((session) => (
                                <button
                                  className={`project-session-node project-session-node--session ${selectionMatches(selection, "session", identity.key, project.id, session.id) ? "is-active" : ""}`}
                                  type="button"
                                  key={session.id}
                                  onClick={() => setSelection({
                                    kind: "session",
                                    identityKey: identity.key,
                                    projectId: project.id,
                                    sessionId: session.id,
                                  })}
                                >
                                  <span className="project-session-node__title">{sessionTitle(session)}</span>
                                  <span className="project-session-node__meta">{session.message_count} 条消息 · {session.updated_at}</span>
                                </button>
                              ))}
                            </div>
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                ) : null}
              </div>
            );
          }) : (
            <div className="workspace-empty-state">
              <strong>暂无会话记录</strong>
              <p>导入真实 Codex 本地 session 后，会按身份和项目出现在这里。</p>
            </div>
          )}
        </div>
        {selectedSessions.length ? (
          <div className="project-session-current-preview">
            <strong>{selectedTitle}</strong>
            <span>{selectedSessions.length} 条当前身份会话</span>
          </div>
        ) : null}
      </article>

      <article className="workspace-card handoff-workspace__history">
        <SectionHeader
          eyebrow="本地会话"
          title="待导入会话"
          description={`从这里选择其他身份或本地 Codex 候选会话，导入到 ${activeIdentityLabel} 的项目记录库。`}
          actions={
            <button
              className="btn btn-primary"
              type="button"
              disabled={importingCodexSessions || selectedCandidateCount === 0 || !activeIdentity}
              onClick={() => void importSelectedCandidates()}
            >
              {importingCodexSessions ? "正在导入" : `导入选中 ${selectedCandidateCount}`}
            </button>
          }
        />
        <div className="project-session-detail-list project-session-import-list">
          {importCandidateIdentities.length ? importCandidateIdentities.map((identity) => {
            const identitySessions = identity.projects.flatMap((project) => project.sessions);
            const identitySelected = identitySessions.every((candidate) => selectedCandidateIds.has(candidate.candidate_id));
            const identityOpen = expandedImportIdentities.has(identity.identityKey);
            return (
              <article className="project-session-import-identity" key={identity.identityKey}>
                <div className="project-session-import-group-head">
                  <button
                    className="project-session-toggle"
                    type="button"
                    aria-expanded={identityOpen}
                    onClick={() => setExpandedImportIdentities((current) => {
                      const next = new Set(current);
                      if (next.has(identity.identityKey)) next.delete(identity.identityKey);
                      else next.add(identity.identityKey);
                      return next;
                    })}
                  >
                    <span aria-hidden="true">{identityOpen ? "▾" : "▸"}</span>
                  </button>
                  <div
                    className="project-session-import-project__head project-session-import-row"
                    role="button"
                    tabIndex={0}
                    onClick={() => toggleImportIdentityCandidates(identitySessions)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        toggleImportIdentityCandidates(identitySessions);
                      }
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={identitySelected}
                      onClick={handleCheckboxClick}
                      onChange={() => toggleImportIdentityCandidates(identitySessions)}
                    />
                    <span>
                      <strong>{identity.identityKindLabel} · {identity.identityLabel}</strong>
                      <small>{identity.projectCount} 个项目 · {identity.sessionCount} 个可导入会话</small>
                    </span>
                  </div>
                </div>
                {identityOpen ? (
                  <div className="project-session-import-projects">
                    {identity.projects.map((project) => {
                      const projectSelected = project.sessions.every((candidate) => selectedCandidateIds.has(candidate.candidate_id));
                      const projectOpen = expandedImportProjects.has(project.key);
                      return (
                        <article className="project-session-import-project" key={project.key}>
                          <div className="project-session-import-group-head">
                            <button
                              className="project-session-toggle"
                              type="button"
                              aria-expanded={projectOpen}
                              onClick={() => setExpandedImportProjects((current) => {
                                const next = new Set(current);
                                if (next.has(project.key)) next.delete(project.key);
                                else next.add(project.key);
                                return next;
                              })}
                            >
                              <span aria-hidden="true">{projectOpen ? "▾" : "▸"}</span>
                            </button>
                            <div
                              className="project-session-import-project__head project-session-import-row"
                              role="button"
                              tabIndex={0}
                              onClick={() => toggleProjectCandidates(project.sessions)}
                              onKeyDown={(event) => {
                                if (event.key === "Enter" || event.key === " ") {
                                  event.preventDefault();
                                  toggleProjectCandidates(project.sessions);
                                }
                              }}
                            >
                              <input
                                type="checkbox"
                                checked={projectSelected}
                                onClick={handleCheckboxClick}
                                onChange={() => toggleProjectCandidates(project.sessions)}
                              />
                              <span>
                                <strong>{project.projectName}</strong>
                                <small>{project.sessions.length} 个会话 · {project.projectPath}</small>
                              </span>
                            </div>
                          </div>
                          {projectOpen ? (
                            <div className="project-session-import-sessions">
                              {project.sessions.map((candidate) => (
                                <div
                                  className="project-session-import-session project-session-import-row"
                                  role="button"
                                  tabIndex={0}
                                  key={candidate.candidate_id}
                                  onClick={() => toggleCandidate(candidate.candidate_id)}
                                  onKeyDown={(event) => {
                                    if (event.key === "Enter" || event.key === " ") {
                                      event.preventDefault();
                                      toggleCandidate(candidate.candidate_id);
                                    }
                                  }}
                                >
                                  <input
                                    type="checkbox"
                                    checked={selectedCandidateIds.has(candidate.candidate_id)}
                                    onClick={handleCheckboxClick}
                                    onChange={() => toggleCandidate(candidate.candidate_id)}
                                  />
                                  <span className="project-session-import-session__body">
                                    <span className="project-session-import-session__title">{candidate.title}</span>
                                    <span className="project-session-import-session__meta">
                                      {candidate.message_count} 条消息 · {candidate.updated_at}
                                    </span>
                                  </span>
                                  <span className="status-tag neutral">codex_local</span>
                                </div>
                              ))}
                            </div>
                          ) : null}
                        </article>
                      );
                    })}
                  </div>
                ) : null}
              </article>
            );
          }) : (
            <div className="workspace-empty-state">
              <strong>暂无可导入会话</strong>
              <p>当前本地 Codex 索引中没有其他身份或未导入的候选会话。</p>
            </div>
          )}
        </div>
      </article>
    </section>
  );
}
