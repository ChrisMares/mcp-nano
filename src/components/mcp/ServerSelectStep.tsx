import React from "react";
import CustomSelect from "@/components/ui/CustomSelect";
import type { SelectOption } from "@/components/ui/CustomSelect";
import type { McpServer } from "@/types/mcp";
import { ArrowRight } from "lucide-react";
import { fieldLabel, textInput, alertValidation, wizardNav, btnPrimary, btnSecondary } from "@/styles/classes";

interface ServerSelectStepProps {
  serverMode: "new" | string;
  serverName: string;
  serverNameError: string | null;
  existingServers: McpServer[];
  onServerModeChange: (mode: "new" | string) => void;
  onServerNameChange: (name: string) => void;
  onNext: () => void;
}

const ServerSelectStep: React.FC<ServerSelectStepProps> = ({
  serverMode,
  serverName,
  serverNameError,
  existingServers,
  onServerModeChange,
  onServerNameChange,
  onNext,
}) => {
  const hasServers = existingServers.length > 0;
  const isCreating = serverMode === "new";
  const canNext = isCreating ? serverName.trim().length > 0 && !serverNameError : serverMode !== "new";

  return (
    <div>
      <h2 className="text-lg font-semibold text-foreground mb-1">Choose an MCP Server</h2>
      <p className="text-sm text-muted-foreground mb-5">
        {hasServers
          ? "Select an existing server to add a tool to, or create a new one."
          : "Name your first MCP server to get started."}
      </p>

      <div className="space-y-4 max-w-md">
        {hasServers && !isCreating && (
          <>
            <div>
              <label className={fieldLabel}>Server</label>
              <CustomSelect
                value={serverMode}
                onChange={onServerModeChange}
                options={existingServers.map((s): SelectOption => ({ value: s.id, label: s.name }))}
                placeholder="Select a server"
              />
            </div>
            <button
              type="button"
              className={btnSecondary}
              onClick={() => { onServerModeChange("new"); onServerNameChange(""); }}
            >
              + Create New Server
            </button>
          </>
        )}

        {isCreating && (
          <div>
            <label className={fieldLabel}>Server Name</label>
            <input
              type="text"
              value={serverName}
              onChange={(e) => onServerNameChange(e.target.value)}
              placeholder="e.g. MyProjectMcp"
              className={textInput}
            />
            {serverNameError && <p className={alertValidation}>{serverNameError}</p>}
            <p className="text-xs text-muted-foreground mt-1">Alphanumeric and underscores only, no spaces.</p>
            {hasServers && (
              <button
                type="button"
                className="text-xs text-muted-foreground hover:text-foreground mt-2 underline transition-colors"
                onClick={() => onServerModeChange(existingServers[0].id)}
              >
                Back to server list
              </button>
            )}
          </div>
        )}
      </div>

      <div className={wizardNav}>
        <div />
        <button type="button" disabled={!canNext} onClick={onNext} className={btnPrimary}>
          Create Tool <ArrowRight size={16} />
        </button>
      </div>
    </div>
  );
};

export default ServerSelectStep;
