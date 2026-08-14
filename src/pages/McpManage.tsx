import React, { useState, useEffect, useCallback } from "react";
import { useNavigate, Link } from "react-router-dom";
import { useScopeOptions } from "@/hooks/useScopeOptions";
import {
  getMcpServers,
  getMcpServer,
  createMcpTool,
  updateMcpTool,
  deleteMcpTool,
  toggleMcpTool,
  deleteMcpServer,
} from "@/utils/apicalls";
import { Loader2, Trash2, Plus, Cable, ArrowRight } from "lucide-react";
import PageHead from "@/components/shared/PageHead";
import ExpandableCard from "@/components/mcp/ExpandableCard";
import ToolWizard from "@/components/mcp/ToolWizard";
import NumberStepper from "@/components/ui/NumberStepper";
import type { McpServer, ToolDefinition, ToolFormData } from "@/types/mcp";
import { toolFormToPayload, toolToFormData, DEFAULT_MAX_CHUNK_LIMIT } from "@/types/mcp";
import { card, badge, btnDanger, btnPrimary, btnSecondary, btnDeleteSmall, alertSuccess, alertError, loader, modalOverlay, modalPanel, confirmInput, btnCancel, btnConfirm } from "@/styles/classes";

type ManageMode = "list" | "add" | "edit";

const McpManage: React.FC = () => {
  const navigate = useNavigate();
  const { repoOptions, groupOptions, websiteOptions } = useScopeOptions();
  const [servers, setServers] = useState<McpServer[]>([]);
  const [expandedServerId, setExpandedServerId] = useState<string | null>(null);
  const [manageMode, setManageMode] = useState<ManageMode>("list");
  const [selectedTool, setSelectedTool] = useState<ToolDefinition | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [deleteConfirmServerId, setDeleteConfirmServerId] = useState<string | null>(null);
  const [deleteConfirmText, setDeleteConfirmText] = useState("");
  const [deleteToolPending, setDeleteToolPending] = useState<ToolDefinition | null>(null);
  const [savingMaxChunkToolId, setSavingMaxChunkToolId] = useState<string | null>(null);

  const fetchServers = useCallback(async () => {
    try {
      const res = await getMcpServers();
      const list: McpServer[] = res?.servers || [];
      setServers(list);
      // fetch tool counts for all servers upfront
      const details = await Promise.allSettled(
        list.map((s) => getMcpServer(s.id))
      );
      setServers(
        list.map((s, i) => {
          const r = details[i];
          return r.status === "fulfilled" ? r.value?.server ?? s : s;
        })
      );
    } catch {
      setServers([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchServerDetail = useCallback(async (serverId: string) => {
    try {
      const res = await getMcpServer(serverId);
      const server: McpServer = res?.server;
      if (server) setServers((prev) => prev.map((s) => (s.id === serverId ? server : s)));
    } catch { /* keep existing data */ }
  }, []);

  useEffect(() => {
    fetchServers();
  }, [fetchServers]);

  // Auto-expand when there's exactly one server
  useEffect(() => {
    if (!loading && servers.length === 1) setExpandedServerId(servers[0].id);
  }, [loading, servers]);

  const resetToList = () => {
    setManageMode("list");
    setSelectedTool(null);
    setError(null);
    setSuccess(false);
  };

  const toggleServer = (serverId: string) => {
    if (expandedServerId === serverId) {
      setExpandedServerId(null);
      resetToList();
    } else {
      setExpandedServerId(serverId);
      resetToList();
      fetchServerDetail(serverId);
    }
  };

  const handleAddTool = useCallback(async (data: ToolFormData) => {
    if (!expandedServerId) return;
    setSaving(true);
    setError(null);
    setSuccess(false);
    try {
      await createMcpTool(expandedServerId, toolFormToPayload(data));
      setSuccess(true);
      setTimeout(() => setSuccess(false), 4000);
      await fetchServerDetail(expandedServerId);
      resetToList();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : typeof err === "string" ? err : "Failed to add tool");
    } finally {
      setSaving(false);
    }
  }, [expandedServerId, fetchServerDetail]);

  const handleUpdateTool = useCallback(async (data: ToolFormData) => {
    if (!expandedServerId || !selectedTool) return;
    setSaving(true);
    setError(null);
    setSuccess(false);
    try {
      await updateMcpTool(expandedServerId, selectedTool.id, toolFormToPayload(data));
      setSuccess(true);
      setTimeout(() => setSuccess(false), 4000);
      await fetchServerDetail(expandedServerId);
      resetToList();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to update tool");
    } finally {
      setSaving(false);
    }
  }, [expandedServerId, selectedTool, fetchServerDetail]);

  const handleDeleteTool = useCallback(() => {
    if (!selectedTool) return;
    setDeleteToolPending(selectedTool);
  }, [selectedTool]);

  const handleDeleteToolById = useCallback((toolId: string) => {
    const tool = servers.find((s) => s.id === expandedServerId)?.tools?.find((t) => t.id === toolId) ?? null;
    setDeleteToolPending(tool);
  }, [servers, expandedServerId]);

  const confirmDeleteTool = useCallback(async () => {
    if (!expandedServerId || !deleteToolPending) return;
    setError(null);
    try {
      await deleteMcpTool(expandedServerId, deleteToolPending.id);
      await fetchServerDetail(expandedServerId);
      resetToList();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to delete tool");
    } finally {
      setDeleteToolPending(null);
    }
  }, [expandedServerId, deleteToolPending, fetchServerDetail]);

  const handleToggleToolActive = useCallback(async (tool: ToolDefinition) => {
    if (!expandedServerId) return;
    setError(null);
    try {
      await toggleMcpTool(expandedServerId, tool.id, !tool.active);
      await fetchServerDetail(expandedServerId);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to toggle tool");
    }
  }, [expandedServerId, fetchServerDetail]);

  const handleUpdateMaxChunkLimit = useCallback(async (tool: ToolDefinition, maxChunkLimit: number) => {
    if (!expandedServerId || maxChunkLimit === (tool.max_chunk_limit ?? DEFAULT_MAX_CHUNK_LIMIT)) return;
    setError(null);
    setSavingMaxChunkToolId(tool.id);
    try {
      await updateMcpTool(expandedServerId, tool.id, {
        name: tool.name,
        description: tool.description ?? "",
        code_search_scopes: tool.code_search_scopes.map((s) => ({
          collection: s.collection,
          repo_names: s.repo_names,
        })),
        document_search_scopes: tool.document_search_scopes.map((s) => ({
          collection: s.collection,
          group_ids: s.group_ids,
        })),
        max_chunk_limit: maxChunkLimit,
      });
      await fetchServerDetail(expandedServerId);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to update max chunk limit");
    } finally {
      setSavingMaxChunkToolId(null);
    }
  }, [expandedServerId, fetchServerDetail]);

  const handleDeleteServer = useCallback((serverId: string) => {
    setDeleteConfirmServerId(serverId);
    setDeleteConfirmText("");
  }, []);

  const confirmDeleteServer = useCallback(async () => {
    if (!deleteConfirmServerId) return;
    setError(null);
    try {
      await deleteMcpServer(deleteConfirmServerId);
      if (expandedServerId === deleteConfirmServerId) {
        setExpandedServerId(null);
        resetToList();
      }
      await fetchServers();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to delete server");
    } finally {
      setDeleteConfirmServerId(null);
      setDeleteConfirmText("");
    }
  }, [deleteConfirmServerId, expandedServerId, fetchServers]);

  const expandedServer = servers.find((s) => s.id === expandedServerId);
  const tools = expandedServer?.tools || [];

  return (
    <div className={card}>
      <PageHead
        title="Manage MCP Servers"
        description="Manage MCP servers and tools."
      />
      <h1 className="text-2xl font-bold text-foreground mb-2">Manage MCP Servers</h1>
      <p className="text-muted-foreground mb-6">
        Expand a server to view, add, and edit its tools, or delete servers and tools you no longer need.
      </p>

      {error && <div className={alertError}>{error}</div>}
      {success && <div className={alertSuccess}>Tool saved!</div>}

      {loading ? (
        <div className={loader}>
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading servers...
        </div>
      ) : servers.length === 0 ? (
        <div className="text-center py-8">
          <p className="text-muted-foreground mb-2">You haven't created any MCP servers yet.</p>
          <p className="text-sm text-muted-foreground mb-6">Create a server to add MCP tools.</p>
          <Link to="/mcp/create" className={`${btnPrimary} inline-flex`}>
            Create MCP Server
            <ArrowRight size={16} />
          </Link>
        </div>
      ) : (
        <div className="max-w-3xl space-y-3">
          {servers.map((server) => (
            <ExpandableCard
              key={server.id}
              title={server.name}
              subtitle={server.description ?? undefined}
              expanded={expandedServerId === server.id}
              onToggle={() => toggleServer(server.id)}
              badge={
                <span className={badge}>
                  {server.tools?.length ?? 0} tool{server.tools?.length === 1 ? "" : "s"}
                </span>
              }
              actions={
                <>
                  <button onClick={() => navigate("/mcp/connect")} className={btnSecondary}>
                    <Cable size={14} /> Connect
                  </button>
                  <button onClick={() => handleDeleteServer(server.id)} className={btnDanger}>
                    <Trash2 size={14} /> Delete
                  </button>
                </>
              }
            >
              {/* Tool list view */}
              {manageMode === "list" && (
                <div className="space-y-3">
                  {tools.length === 0 ? (
                    <div className="text-center py-4">
                      <p className="text-sm text-muted-foreground mb-3">This server doesn't have any tools yet.</p>
                      <button onClick={() => setManageMode("add")} className={`${btnPrimary} inline-flex mx-auto`}>
                        Create Tool
                        <ArrowRight size={16} />
                      </button>
                    </div>
                  ) : (
                    <div className="grid gap-2">
                      {tools.map((tool) => (
                        <div
                          key={tool.id}
                          className="w-full p-3 rounded-lg border border-border"
                        >
                          <div className="flex items-start justify-between gap-2">
                            <div className="min-w-0">
                              <p className="font-medium text-foreground text-sm">{tool.name}</p>
                              {tool.description && (
                                <p className="text-xs text-muted-foreground mt-0.5">{tool.description}</p>
                              )}
                              <div className="flex gap-3 mt-1 text-xs text-muted-foreground">
                                {tool.code_search_scopes.length > 0 && (
                                  <span>{tool.code_search_scopes.flatMap((s) => s.repo_names).length} repos</span>
                                )}
                                {tool.document_search_scopes.length > 0 && (
                                  <span>{tool.document_search_scopes.flatMap((s) => s.group_ids).length} groups</span>
                                )}
                                <span className={tool.active ? "text-success" : ""}>{tool.active ? "Active" : "Inactive"}</span>
                              </div>
                            </div>
                            <div className="flex items-center gap-1.5 shrink-0">
                              <div className="flex items-center gap-1">
                                <span className="text-xs text-muted-foreground">Max Chunks</span>
                                <NumberStepper
                                  size="sm"
                                  value={tool.max_chunk_limit ?? DEFAULT_MAX_CHUNK_LIMIT}
                                  onChange={(value) => handleUpdateMaxChunkLimit(tool, value)}
                                  min={1}
                                  max={50}
                                  disabled={savingMaxChunkToolId === tool.id}
                                />
                              </div>
                              <button
                                onClick={() => handleToggleToolActive(tool)}
                                className={`${btnSecondary} px-3 py-1.5 text-xs`}
                              >
                                {tool.active ? "Disable" : "Enable"}
                              </button>
                              <button
                                onClick={() => { setSelectedTool(tool); setManageMode("edit"); }}
                                className={`${btnSecondary} px-3 py-1.5 text-xs`}
                              >
                                Edit
                              </button>
                              <button
                                onClick={() => handleDeleteToolById(tool.id)}
                                className={`${btnDeleteSmall} px-3 py-1.5 text-xs`}
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}

                  <button
                    onClick={() => setManageMode("add")}
                    className={btnPrimary}
                  >
                    <Plus size={16} /> Add tools to this MCP
                  </button>
                </div>
              )}

              {/* Add tool wizard */}
              {manageMode === "add" && (
                <ToolWizard
                  repoOptions={repoOptions}
                  groupOptions={groupOptions}
                  websiteOptions={websiteOptions}
                  onSave={handleAddTool}
                  saving={saving}
                  saveLabel="Add Tool"
                  onCancel={resetToList}
                  editMode
                />
              )}

              {/* Edit tool wizard */}
              {manageMode === "edit" && selectedTool && (
                <ToolWizard
                  key={selectedTool.id}
                  repoOptions={repoOptions}
                  groupOptions={groupOptions}
                  websiteOptions={websiteOptions}
                  initialData={toolToFormData(selectedTool, websiteOptions)}
                  onSave={handleUpdateTool}
                  saving={saving}
                  saveLabel="Save Changes"
                  onCancel={resetToList}
                  onDelete={handleDeleteTool}
                  editMode
                  toolName={selectedTool.name}
                />
              )}
            </ExpandableCard>
          ))}
        </div>
      )}

      {/* Delete tool confirm modal */}
      {deleteToolPending && (
        <div className={modalOverlay}>
          <div className={modalPanel}>
            <h3 className="text-lg font-semibold text-foreground mb-2">Delete Tool</h3>
            <p className="text-sm text-muted-foreground mb-4">
              Are you sure you want to delete the tool{" "}
              <span className="font-medium text-foreground">{deleteToolPending.name}</span>?
              This cannot be undone.
            </p>
            <div className="flex justify-end gap-3">
              <button onClick={() => setDeleteToolPending(null)} className={btnCancel}>
                Cancel
              </button>
              <button onClick={confirmDeleteTool} className={btnConfirm}>
                Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Type-to-confirm delete server modal */}
      {deleteConfirmServerId && (
        <div className={modalOverlay}>
          <div className={modalPanel}>
            <h3 className="text-lg font-semibold text-foreground mb-2">Delete Server</h3>
            <p className="text-sm text-muted-foreground mb-4">
              You are about to delete the server{" "}
              <span className="font-medium text-foreground">
                {servers.find((s) => s.id === deleteConfirmServerId)?.name}
              </span>{" "}
              and all its tools. If you are sure, type{" "}
              <span className="font-medium text-foreground">'delete'</span>
            </p>
            <input
              type="text"
              value={deleteConfirmText}
              onChange={(e) => setDeleteConfirmText(e.target.value)}
              placeholder="Type delete to confirm"
              className={confirmInput}
            />
            <div className="flex justify-end gap-3">
              <button onClick={() => { setDeleteConfirmServerId(null); setDeleteConfirmText(""); }} className={btnCancel}>
                Cancel
              </button>
              <button
                onClick={confirmDeleteServer}
                disabled={deleteConfirmText.toLowerCase() !== "delete"}
                className={btnConfirm}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default McpManage;
