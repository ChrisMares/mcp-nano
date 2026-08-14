import React, { useState, useEffect, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { useScopeOptions } from "@/hooks/useScopeOptions";
import { useToolForm } from "@/hooks/useToolForm";
import { getMcpServers, createMcpServer, createMcpTool } from "@/utils/apicalls";
import PageHead from "@/components/shared/PageHead";
import StepIndicator from "@/components/shared/StepIndicator";
import ServerSelectStep from "@/components/mcp/ServerSelectStep";
import DataSelectStep from "@/components/mcp/DataSelectStep";
import ToolDetailsStep from "@/components/mcp/ToolDetailsStep";
import type { McpServer } from "@/types/mcp";
import { toolFormToPayload } from "@/types/mcp";
import { alertError } from "@/styles/classes";

const SERVER_NAME_RE = /^[A-Za-z0-9_]+$/;
const STEP_LABELS = ["MCP Server", "Select Data", "Tool Details"];

const validateServerName = (name: string, existingNames: string[] = []): string | null => {
  if (!name.trim()) return "Server name is required";
  if (!SERVER_NAME_RE.test(name)) return "Only letters, numbers, and underscores allowed";
  if (existingNames.some((n) => n.toLowerCase() === name.toLowerCase())) {
    return "Server name already exists";
  }
  return null;
};

const McpCreate: React.FC = () => {
  const navigate = useNavigate();
  const { repoOptions, groupOptions, websiteOptions } = useScopeOptions();
  const { form, toggleRepo, toggleGroup, setRepos, setGroups, toggleWebsite, setWebsites, updateForm, setMaxChunkLimit } = useToolForm();
  const [step, setStep] = useState(1);

  // Step 1 state
  const [serverMode, setServerMode] = useState<"new" | string>("new");
  const [serverName, setServerName] = useState("");
  const [serverNameError, setServerNameError] = useState<string | null>(null);
  const [existingServers, setExistingServers] = useState<McpServer[]>([]);

  // Save state
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getMcpServers()
      .then((res) => {
        const servers: McpServer[] = res?.servers || [];
        setExistingServers(servers);
        if (servers.length > 0) setServerMode(servers[0].id);
      })
      .catch(() => setExistingServers([]));
  }, []);

  const existingNames = existingServers.map((s) => s.name);

  useEffect(() => {
    if (serverName) {
      setServerNameError(
        validateServerName(
          serverName,
          existingServers.map((s) => s.name),
        ),
      );
    }
  }, [existingServers, serverName]);

  const handleServerNameChange = (value: string) => {
    const cleaned = value.replace(/\s/g, "");
    setServerName(cleaned);
    setServerNameError(validateServerName(cleaned, existingNames));
  };

  const handleStep1Next = () => {
    if (serverMode === "new") {
      const err = validateServerName(serverName, existingNames);
      if (err) { setServerNameError(err); return; }
    }
    setStep(2);
  };

  const handleSave = useCallback(async () => {
    if (!form.name.trim()) return;
    setSaving(true);
    setError(null);

    try {
      let serverId: string;
      if (serverMode === "new") {
        const serverRes = await createMcpServer(serverName);
        serverId = serverRes.server.id;
        setExistingServers((prev) => [...prev, serverRes.server]);
      } else {
        serverId = serverMode;
      }

      await createMcpTool(serverId, toolFormToPayload(form));
      navigate("/mcp/manage");
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : typeof err === "string" ? err : "Failed to save");
    } finally {
      setSaving(false);
    }
  }, [form, serverMode, serverName, navigate]);

  return (
    <div className="rounded-lg border border-border bg-card p-6">
      <PageHead
        title="Create MCP Server"
        description="Create an MCP server and define tool scopes for your data."
      />
      <h1 className="text-2xl font-bold text-foreground mb-1">Create MCP Tool</h1>
      <p className="text-muted-foreground mb-6 text-sm">
        Define a tool name, description, and data scope.
      </p>

      {error && <div className={alertError}>{error}</div>}

      <StepIndicator current={step} total={3} labels={STEP_LABELS} onStepClick={(s) => setStep(s)} />

      <div className="max-w-2xl mx-auto">
        {step === 1 && (
          <ServerSelectStep
            serverMode={serverMode}
            serverName={serverName}
            serverNameError={serverNameError}
            existingServers={existingServers}
            onServerModeChange={setServerMode}
            onServerNameChange={handleServerNameChange}
            onNext={handleStep1Next}
          />
        )}

        {step === 2 && (
          <DataSelectStep
            repoOptions={repoOptions}
            groupOptions={groupOptions}
            websiteOptions={websiteOptions}
            selectedRepos={form.selectedRepos}
            selectedGroups={form.selectedGroups}
            selectedWebsites={form.selectedWebsites}
            onToggleRepo={toggleRepo}
            onToggleGroup={toggleGroup}
            onToggleWebsite={toggleWebsite}
            onSetRepos={setRepos}
            onSetGroups={setGroups}
            onSetWebsites={setWebsites}
            onBack={() => setStep(1)}
            onNext={() => setStep(3)}
          />
        )}

        {step === 3 && (
          <ToolDetailsStep
            name={form.name}
            description={form.description}
            selectedRepos={form.selectedRepos}
            selectedGroups={form.selectedGroups}
            selectedWebsites={form.selectedWebsites}
            maxChunkLimit={form.maxChunkLimit}
            onNameChange={(name) => updateForm({ name })}
            onDescriptionChange={(description) => updateForm({ description })}
            onMaxChunkLimitChange={setMaxChunkLimit}
            onBack={() => setStep(2)}
            onSave={handleSave}
            saving={saving}
            saveLabel="Create Tool"
          />
        )}
      </div>
    </div>
  );
};

export default McpCreate;
