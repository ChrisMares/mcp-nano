import React from "react";
import StepIndicator from "@/components/shared/StepIndicator";
import DataSelectStep from "./DataSelectStep";
import ToolDetailsStep from "./ToolDetailsStep";
import { useToolForm } from "@/hooks/useToolForm";
import type { ToolFormData } from "@/types/mcp";
import { btnDanger, btnCancel } from "@/styles/classes";
import { Trash2 } from "lucide-react";
import { useState } from "react";

const STEP_LABELS = ["Select Data", "Tool Details"];

interface ToolWizardProps {
  repoOptions: string[];
  groupOptions: string[];
  websiteOptions: string[];
  initialData?: ToolFormData;
  onSave: (data: ToolFormData) => void;
  saving: boolean;
  saveLabel: string;
  onCancel: () => void;
  onDelete?: () => void;
  /** When true: single-page layout with tool name header, no step indicator */
  editMode?: boolean;
  /** Tool name shown in the edit mode header */
  toolName?: string;
}

const ToolWizard: React.FC<ToolWizardProps> = ({
  repoOptions,
  groupOptions,
  websiteOptions,
  initialData,
  onSave,
  saving,
  saveLabel,
  onCancel,
  onDelete,
  editMode = false,
  toolName,
}) => {
  const [step, setStep] = useState(1);
  const { form, toggleRepo, toggleGroup, setRepos, setGroups, toggleWebsite, setWebsites, updateForm, setMaxChunkLimit } = useToolForm(initialData);

  if (editMode) {
    return (
      <div className="space-y-4">
        {/* Header: tool name + cancel */}
        <div className="flex items-center justify-between pb-2 border-b border-border">
          <h3 className="font-semibold text-foreground">
            {toolName ? (
              <>Editing: <span className="text-primary font-mono">{toolName}</span></>
            ) : (
              "Add Tool"
            )}
          </h3>
          <div className="flex items-center gap-2">
            {onDelete && (
              <button type="button" onClick={onDelete} className={btnDanger}>
                <Trash2 size={14} /> Delete Tool
              </button>
            )}
            <button type="button" onClick={onCancel} className={btnCancel}>
              Cancel
            </button>
          </div>
        </div>

        {/* Data selection */}
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
          editMode
          onNext={() => {}}
        />

        {/* Name & description */}
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
          onBack={onCancel}
          onSave={() => onSave(form)}
          saving={saving}
          saveLabel={saveLabel}
          editMode
        />
      </div>
    );
  }

  // --- create flow: original wizard ---
  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <button type="button" onClick={onCancel} className="text-sm text-muted-foreground hover:text-foreground transition-colors">
          &larr; Back to tool list
        </button>
      </div>

      <StepIndicator current={step} total={2} labels={STEP_LABELS} onStepClick={(s) => setStep(s)} />

      {step === 1 && (
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
          onNext={() => setStep(2)}
        />
      )}

      {step === 2 && (
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
          onBack={() => setStep(1)}
          onSave={() => onSave(form)}
          saving={saving}
          saveLabel={saveLabel}
        />
      )}
    </div>
  );
};

export default ToolWizard;
