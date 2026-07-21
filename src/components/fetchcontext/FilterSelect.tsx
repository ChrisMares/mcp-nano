import React from "react";
import { fieldLabel } from "@/styles/classes";

interface Props {
  label: string;
  options: string[];
  selected: Set<string>;
  onToggle: (name: string) => void;
}

const FilterSelect: React.FC<Props> = ({ label, options, selected, onToggle }) => (
  <div>
    <label className={fieldLabel}>
      {label} <span className="text-muted-foreground font-normal">(none = query all)</span>
    </label>
    <div className="border border-border rounded-md bg-background max-h-40 overflow-y-auto p-2 space-y-1">
      {options.map((name) => (
        <label key={name} className="flex items-center gap-2 cursor-pointer px-2 py-1 rounded hover:bg-muted/50">
          <input
            type="checkbox"
            checked={selected.has(name)}
            onChange={() => onToggle(name)}
            className="w-3.5 h-3.5 rounded border-border text-primary focus:ring-primary"
          />
          <span className="text-sm text-foreground truncate">{name}</span>
        </label>
      ))}
    </div>
  </div>
);

export default FilterSelect;
