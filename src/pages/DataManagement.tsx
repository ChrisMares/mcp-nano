import React, { useState, useEffect, useCallback, useMemo } from "react";
import {
  getFiles,
  getWebsites,
  deleteRepo,
  deleteDocument,
  deleteGroup,
  clearUserCollection,
  deleteWebsite,
  deleteWebsiteGroup,
  clearWebsites,
} from "@/utils/apicalls";
import { Trash } from "@/icons";
import PageHead from "@/components/shared/PageHead";
import type { UserFiles, WebsiteItem } from "@/types/embed";
import {
  btnDelete,
  btnDeleteSmall,
  btnIconDelete,
  btnCancel,
  btnConfirm,
  modalOverlay,
  modalPanel,
  confirmInput,
  cellBase,
  thCell,
  alertError,
} from "@/styles/classes";
import { formatBytes } from "@/utils/format-bytes";

interface ItemDeleteTarget {
  type: "repo" | "document" | "website";
  id: string;
  displayName: string;
}

interface TypeConfirmTarget {
  kind: "collection" | "group" | "website-group";
  key: string;
  title: string;
  message: string;
}

const stripGuidPrefix = (name: string) =>
  name.replace(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}_/, "");

const sortGroupsDefaultFirst = (names: string[]) =>
  names.sort((a, b) => {
    if (a === "default") return -1;
    if (b === "default") return 1;
    return a.localeCompare(b);
  });

const groupByName = <T extends { group: string }>(items: T[]) => {
  const groups: Record<string, T[]> = {};
  for (const item of items) (groups[item.group || "default"] ??= []).push(item);
  return groups;
};

const formatDate = (iso: string) => {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso.slice(0, 10);
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
};

const DataManagement: React.FC = () => {
  const [userFiles, setUserFiles] = useState<UserFiles | null>(null);
  const [websites, setWebsites] = useState<WebsiteItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [deletingKey, setDeletingKey] = useState<string | null>(null);
  const [itemDeleteTarget, setItemDeleteTarget] = useState<ItemDeleteTarget | null>(null);
  const [typeConfirmTarget, setTypeConfirmTarget] = useState<TypeConfirmTarget | null>(null);
  const [typeConfirmText, setTypeConfirmText] = useState("");
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);

  const fetchFiles = useCallback(async () => {
    try {
      const res = await getFiles();
      setUserFiles(res);
    } catch (err) {
      setError(`Failed to load files: ${String(err)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchWebsites = useCallback(async () => {
    try {
      const res = await getWebsites();
      setWebsites(res.websites ?? []);
    } catch (err) {
      setError(`Failed to load websites: ${String(err)}`);
    }
  }, []);

  useEffect(() => {
    fetchFiles();
    fetchWebsites();
  }, [fetchFiles, fetchWebsites]);

  const runDelete = async (key: string, action: () => Promise<void>, close: () => void) => {
    setDeletingKey(key);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(`Delete failed: ${String(err)}`);
    } finally {
      setDeletingKey(null);
      close();
    }
  };

  const handleDeleteItem = async () => {
    if (!itemDeleteTarget) return;
    const { type, id } = itemDeleteTarget;
    await runDelete(`${type}:${id}`, async () => {
      if (type === "website") {
        await deleteWebsite(id);
        await fetchWebsites();
        return;
      }
      await (type === "repo" ? deleteRepo(id) : deleteDocument(id));
      await fetchFiles();
    }, () => setItemDeleteTarget(null));
  };

  const handleTypeConfirmDelete = async () => {
    if (!typeConfirmTarget) return;
    const { kind, key } = typeConfirmTarget;
    await runDelete(`${kind}:${key}`, async () => {
      if (kind === "website-group") {
        await (key === "__all__" ? clearWebsites() : deleteWebsiteGroup(key));
        await fetchWebsites();
        return;
      }
      await (kind === "collection" ? clearUserCollection(key) : deleteGroup(key));
      await fetchFiles();
    }, () => {
      setTypeConfirmTarget(null);
      setTypeConfirmText("");
    });
  };

  const openCollectionClear = (collection: "codebase" | "general", label: string) =>
    setTypeConfirmTarget({
      kind: "collection",
      key: collection,
      title: `Delete All ${label}`,
      message: "You are about to delete your entire collection.",
    });

  const openGroupDelete = (groupName: string) =>
    setTypeConfirmTarget({
      kind: "group",
      key: groupName,
      title: "Delete Group",
      message: `You are about to delete all documents in group '${groupName}'.`,
    });

  const openWebsiteGroupDelete = (groupName: string) =>
    setTypeConfirmTarget({
      kind: "website-group",
      key: groupName,
      title: "Delete Website Group",
      message: `You are about to delete all website embeddings in group '${groupName}'. Documents in this group will NOT be affected.`,
    });

  const documentsByGroup = useMemo(
    () =>
      groupByName(
        (userFiles?.documents ?? []).map((doc) => ({
          ...doc,
          file_type: doc.file_type && doc.file_type.slice(doc.file_type.lastIndexOf("/") + 1),
        })),
      ),
    [userFiles?.documents],
  );

  const sortedGroupNames = useMemo(
    () => sortGroupsDefaultFirst(Object.keys(documentsByGroup)),
    [documentsByGroup],
  );

  const websitesByGroup = useMemo(() => groupByName(websites), [websites]);

  const sortedWebsiteGroups = useMemo(
    () => sortGroupsDefaultFirst(Object.keys(websitesByGroup)),
    [websitesByGroup],
  );

  return (
    <div className="max-w-[788px] space-y-4">
      <div className="rounded-lg border border-border bg-card p-4">
        <PageHead
          title="Manage Embedded Data"
          description="View and manage your embedded documents and code repositories. Delete or update vector embeddings for your RAG pipeline."
        />
        <h1 className="text-2xl font-bold text-foreground mb-1">Data Management</h1>
        <p className="text-sm text-muted-foreground">View and delete your embedded data.</p>
      </div>

      {error && (
        <div className={alertError} role="alert">
          {error}
        </div>
      )}

      {loading ? (
        <div className="rounded-lg border border-border bg-card p-12 text-center text-muted-foreground">
          Loading...
        </div>
      ) : (
        <div className="space-y-4">
          {/* Repos */}
          <div className="rounded-lg border border-border bg-card p-4 flex flex-col">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-lg font-semibold text-foreground">Repos</h2>
              <button onClick={() => openCollectionClear("codebase", "Codebase")} className={btnDelete}>
                Delete All
              </button>
            </div>
            {userFiles?.repos.length ? (
              <div className="overflow-y-auto max-h-[60vh]">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border">
                      <th className={thCell}>Name</th>
                      <th className={thCell}>Date</th>
                      <th className={`text-right ${cellBase}`}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {userFiles.repos.map((repo) => (
                      <tr key={repo.repo_name} className="border-b border-border last:border-0">
                        <td className={`${cellBase} text-foreground`}>
                          <span className="flex items-center gap-2">
                            <span className="inline-block w-2 h-2 rounded-full bg-info shrink-0" />
                            {repo.repo_name}
                          </span>
                        </td>
                        <td className={`${cellBase} text-muted-foreground whitespace-nowrap`}>{formatDate(repo.created_at ?? "")}</td>
                        <td className={`${cellBase} text-right`}>
                          <button
                            onClick={() => setItemDeleteTarget({ type: "repo", id: repo.repo_name, displayName: repo.repo_name })}
                            disabled={deletingKey === `repo:${repo.repo_name}`}
                            className={btnIconDelete}
                          >
                            {deletingKey === `repo:${repo.repo_name}` ? "..." : <Trash />}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground py-4 text-center">No repos embedded yet.</p>
            )}
          </div>

          {/* Documents */}
          <div className="rounded-lg border border-border bg-card p-4 flex flex-col">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-lg font-semibold text-foreground">Documents</h2>
              <button onClick={() => openCollectionClear("general", "Documents")} className={btnDelete}>
                Delete All
              </button>
            </div>
            {sortedGroupNames.length ? (
              <div className="space-y-3 overflow-y-auto max-h-[60vh]">
                {sortedGroupNames.map((groupName) => {
                  const isOpen = !!expandedGroups[groupName];
                  return (
                    <div key={groupName} className="rounded-md border border-border bg-background">
                      <div className="flex items-center justify-between px-3 py-2">
                        <button
                          onClick={() => setExpandedGroups((prev) => ({ ...prev, [groupName]: !isOpen }))}
                          className="flex items-center gap-2 text-sm font-medium text-foreground focus:outline-none"
                        >
                          <span className="text-muted-foreground">{isOpen ? "▾" : "▸"}</span>
                          <span>{groupName}</span>
                        </button>
                        <button onClick={() => openGroupDelete(groupName)} className={btnDeleteSmall}>
                          Delete Group
                        </button>
                      </div>
                      {isOpen && (
                        <div className="px-3 pb-3">
                          <table className="w-full text-sm">
                            <thead>
                              <tr className="border-b border-border">
                                <th className={thCell}>Name</th>
                                <th className={thCell}>Type</th>
                                <th className={thCell}>Size</th>
                                <th className={thCell}>Date</th>
                                <th className={`text-right ${cellBase}`}></th>
                              </tr>
                            </thead>
                            <tbody>
                              {documentsByGroup[groupName].map((doc) => {
                                const name = stripGuidPrefix(doc.filename);
                                const docId = doc.storage_object_id ?? doc.filename;
                                return (
                                  <tr key={docId} className="border-b border-border last:border-0">
                                    <td className={`${cellBase} text-foreground truncate max-w-[120px]`} title={name}>{name}</td>
                                    <td className={`${cellBase} text-muted-foreground`}>{doc.file_type ?? "—"}</td>
                                    <td className={`${cellBase} text-muted-foreground`}>{formatBytes(doc.size_bytes)}</td>
                                    <td className={`${cellBase} text-muted-foreground whitespace-nowrap`}>{formatDate(doc.created_at ?? "")}</td>
                                    <td className={`${cellBase} text-right`}>
                                      <button
                                        onClick={() => {
                                          if (doc.storage_object_id) {
                                            setItemDeleteTarget({ type: "document", id: doc.storage_object_id, displayName: name });
                                          }
                                        }}
                                        disabled={!doc.storage_object_id || deletingKey === `document:${docId}`}
                                        className={btnIconDelete}
                                      >
                                        {deletingKey === `document:${docId}` ? "..." : <Trash />}
                                      </button>
                                    </td>
                                  </tr>
                                );
                              })}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground py-4 text-center">No documents embedded yet.</p>
            )}
          </div>

          {/* Websites */}
          <div className="rounded-lg border border-border bg-card p-4 flex flex-col">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-lg font-semibold text-foreground">Websites</h2>
              <button
                onClick={() => setTypeConfirmTarget({
                  kind: "website-group",
                  key: "__all__",
                  title: "Delete All Websites",
                  message: "You are about to delete ALL website embeddings. Documents will NOT be affected.",
                })}
                className={btnDelete}
              >
                Delete All
              </button>
            </div>
            {sortedWebsiteGroups.length ? (
              <div className="space-y-3 overflow-y-auto max-h-[60vh]">
                {sortedWebsiteGroups.map((groupName) => {
                  const isOpen = !!expandedGroups[`ws-${groupName}`];
                  const groupDate = formatDate(websitesByGroup[groupName].find((w) => w.embedded_at)?.embedded_at ?? "");
                  return (
                    <div key={`ws-${groupName}`} className="rounded-md border border-border bg-background">
                      <div className="flex items-center justify-between px-3 py-2">
                        <button
                          onClick={() => setExpandedGroups((prev) => ({ ...prev, [`ws-${groupName}`]: !isOpen }))}
                          className="flex items-center gap-2 text-sm font-medium text-foreground focus:outline-none"
                        >
                          <span className="text-muted-foreground">{isOpen ? "▾" : "▸"}</span>
                          <span>{groupName}</span>
                          <span className="text-xs text-muted-foreground">({websitesByGroup[groupName].length})</span>
                          <span className="text-xs text-muted-foreground">{groupDate}</span>
                        </button>
                        <button onClick={() => openWebsiteGroupDelete(groupName)} className={btnDeleteSmall}>
                          Delete Group
                        </button>
                      </div>
                      {isOpen && (
                        <div className="px-3 pb-3">
                          <table className="w-full text-sm">
                            <thead>
                              <tr className="border-b border-border">
                                <th className={thCell}>URL</th>
                                <th className={thCell}>Chunks</th>
                                <th className={`text-right ${cellBase}`}></th>
                              </tr>
                            </thead>
                            <tbody>
                              {websitesByGroup[groupName].map((w) => (
                                <tr key={w.url} className="border-b border-border last:border-0">
                                  <td className={`${cellBase} text-foreground break-all`} title={w.url}>{w.url}</td>
                                  <td className={`${cellBase} text-muted-foreground`}>{w.chunk_count}</td>
                                  <td className={`${cellBase} text-right`}>
                                    <button
                                      onClick={() => setItemDeleteTarget({ type: "website", id: w.url, displayName: w.url })}
                                      disabled={deletingKey === `website:${w.url}`}
                                      className={btnIconDelete}
                                    >
                                      {deletingKey === `website:${w.url}` ? "..." : <Trash />}
                                    </button>
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground py-4 text-center">No websites embedded yet.</p>
            )}
          </div>
        </div>
      )}

      {/* Single-click delete confirmation */}
      {itemDeleteTarget && (
        <div className={modalOverlay}>
          <div className={modalPanel}>
            <h3 className="text-lg font-semibold text-foreground mb-2">
              Delete {itemDeleteTarget.type === "repo" ? "Repo" : itemDeleteTarget.type === "website" ? "Website" : "Document"}
            </h3>
            <p className="text-sm text-muted-foreground mb-6">
              Delete{" "}
              <span className="font-medium text-foreground">{itemDeleteTarget.displayName}</span>?
              This will permanently remove the associated embeddings
              {itemDeleteTarget.type !== "website" && " and any stored files"} for this{" "}
              {itemDeleteTarget.type === "repo" ? "repo" : itemDeleteTarget.type === "website" ? "website" : "document"}.
            </p>
            <div className="flex justify-end gap-3">
              <button onClick={() => setItemDeleteTarget(null)} className={btnCancel}>Cancel</button>
              <button onClick={handleDeleteItem} disabled={!!deletingKey} className={btnConfirm}>
                {deletingKey ? "Deleting..." : "Delete"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Type-to-confirm delete (collection clear / group delete) */}
      {typeConfirmTarget && (
        <div className={modalOverlay}>
          <div className={modalPanel}>
            <h3 className="text-lg font-semibold text-foreground mb-2">{typeConfirmTarget.title}</h3>
            <p className="text-sm text-muted-foreground mb-4">
              {typeConfirmTarget.message} If you are sure type{" "}
              <span className="font-medium text-foreground">'delete'</span>
            </p>
            <input
              type="text"
              value={typeConfirmText}
              onChange={(e) => setTypeConfirmText(e.target.value)}
              placeholder="Type delete to confirm"
              className={confirmInput}
            />
            <div className="flex justify-end gap-3">
              <button onClick={() => { setTypeConfirmTarget(null); setTypeConfirmText(""); }} className={btnCancel}>
                Cancel
              </button>
              <button
                onClick={handleTypeConfirmDelete}
                disabled={typeConfirmText.toLowerCase() !== "delete" || !!deletingKey}
                className={btnConfirm}
              >
                {deletingKey === `${typeConfirmTarget.kind}:${typeConfirmTarget.key}` ? "Deleting..." : "Delete"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default DataManagement;
