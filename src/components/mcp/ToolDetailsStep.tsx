import React, { useState } from "react";
import { Loader2 } from "lucide-react";
import { wizardNav, btnPrimary, btnSecondary, fieldLabel, textInput, textArea, alertValidation } from "@/styles/classes";

const TOOL_NAME_RE = /^[A-Za-z0-9_]+$/;

const validateToolName = (name: string): string | null => {
  if (!name.trim()) return null; // show nothing when empty
  if (!TOOL_NAME_RE.test(name)) return "Only letters, numbers, and underscores allowed";
  return null;
};

interface ToolDetailsStepProps {
  name: string;
  description: string;
  selectedRepos: Set<string>;
  selectedGroups: Set<string>;
  selectedWebsites: Set<string>;
  onNameChange: (name: string) => void;
  onDescriptionChange: (desc: string) => void;
  onBack: () => void;
  onSave: () => void;
  saving: boolean;
  saveLabel: string;
  /** When true: hides header, description summary, and Back button */
  editMode?: boolean;
}

const ToolDetailsStep: React.FC<ToolDetailsStepProps> = ({
  name,
  description,
  selectedRepos,
  selectedGroups,
  selectedWebsites,
  onNameChange,
  onDescriptionChange,
  onBack,
  onSave,
  saving,
  saveLabel,
  editMode = false,
}) => {
  const [toolNameError, setToolNameError] = useState<string | null>(null);

  const canSave = name.trim().length > 0 && !validateToolName(name) && !saving;

  const handleToolNameChange = (value: string) => {
    const cleaned = value.replace(/\s/g, "");
    onNameChange(cleaned);
    setToolNameError(validateToolName(cleaned));
  };

  return (
    <div>
      {!editMode && (
        <>
          <h2 className="text-lg font-semibold text-foreground mb-1">Tool Name & Description</h2>
          <p className="text-sm text-muted-foreground mb-5">
            Give your tool a name and description. The LLM reads the description to decide when to call your tool.
          </p>
        </>
      )}

      {/* Data summary — only in wizard flow */}
      {!editMode && (selectedRepos.size > 0 || selectedGroups.size > 0 || selectedWebsites.size > 0) && (
        <div className="mb-5 flex flex-wrap items-center gap-1.5">
          <span className="font-bold text-sm">Selected Data</span>
          {Array.from(selectedRepos).map((r) => (
            <span key={r} className="px-2 py-0.5 bg-primary/10 text-primary text-xs rounded-full">{r}</span>
          ))}
          {Array.from(selectedGroups).map((g) => (
            <span key={g} className="px-2 py-0.5 bg-brand-cyan/15 text-brand-cyan text-xs rounded-full">{g}</span>
          ))}
          {Array.from(selectedWebsites).map((w) => (
            <span key={w} className="px-2 py-0.5 bg-success/15 text-success text-xs rounded-full">{w}</span>
          ))}
        </div>
      )}

      <div className="space-y-4">
        <div>
          <label className={fieldLabel}>Tool Name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => handleToolNameChange(e.target.value)}
            placeholder="e.g. search_backend_code"
            className={textInput}
          />
          {toolNameError && <p className={alertValidation}>{toolNameError}</p>}
          <p className="text-xs text-muted-foreground mt-1">Alphanumeric and underscores only, no spaces.</p>
        </div>

        <div>
          <div className="mb-1">
            <label className="text-sm font-medium text-foreground">Description: Important! </label>
            <p className="text-xs text-muted-foreground mt-0.5">
              Be specific and direct. The LLM uses this description to determine when to call your tool. 
              Focus on what the tool does, what data it is looking up and some hints on when the LLM should use the tool. 
            </p>
          </div>
          <textarea
            value={description}
            onChange={(e) => onDescriptionChange(e.target.value)}
            placeholder="What this tool searches for..."
            rows={3}
            className={textArea}
          />
        </div>
      </div>

      <div className={wizardNav}>
        {!editMode && <button type="button" onClick={onBack} className={btnSecondary}>Back</button>}
        {editMode && <div />}
        <button type="button" disabled={!canSave} onClick={onSave} className={btnPrimary}>
          {saving ? (
            <><Loader2 className="h-4 w-4 animate-spin" /> Saving...</>
          ) : (
            saveLabel
          )}
        </button>
      </div>
    </div>
  );
};

export default ToolDetailsStep;
