import React from "react";
import { ArrowLeft, ArrowRight } from "lucide-react";
import NamePicker from "./NamePicker";
import { wizardNav, btnPrimary, btnSecondary } from "@/styles/classes";

interface Props {
  groupName: string;
  groupMode: "existing" | "new";
  groupOptions: string[];
  onGroupChange: (value: string, mode: "existing" | "new") => void;
  onBack: () => void;
  onNext: () => void;
}

const ConfigureDocsStep: React.FC<Props> = ({
  groupName, groupMode, groupOptions, onGroupChange, onBack, onNext,
}) => (
  <div>
    <h2 className="text-lg font-semibold text-foreground mb-1">Organize Your Documents</h2>
    <p className="text-sm text-muted-foreground mb-5">
      Group similar documents together so you can search across them later. For example,
      create a group for database specs and another for meeting notes.
    </p>

    <div className="mb-6">
      <NamePicker
        label="Group Name"
        required
        options={groupOptions}
        value={groupName}
        mode={groupMode}
        onChange={onGroupChange}
        emptyMessage="No groups yet. Enter a name for your new group."
        newPlaceholder="Enter new group name"
        createLabel="-- Create new group --"
      />
    </div>

    <div className={wizardNav}>
      <button onClick={onBack} className={btnSecondary}>
        <ArrowLeft size={16} /> Back
      </button>
      <button
        disabled={!groupName.trim()}
        onClick={onNext}
        className={btnPrimary}
      >
        Next <ArrowRight size={16} />
      </button>
    </div>
  </div>
);

export default ConfigureDocsStep;
