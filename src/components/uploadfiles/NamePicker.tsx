import React from "react";
import { selectInput, textInput } from "@/styles/classes";

interface Props {
  label: string;
  required?: boolean;
  options: string[];
  value: string;
  mode: "existing" | "new";
  onChange: (value: string, mode: "existing" | "new") => void;
  emptyMessage: string;
  emptyPlaceholder?: string;
  newPlaceholder: string;
  defaultOption?: string;
  createLabel?: string;
}

// Shared select-or-create-new picker used for repo names and group names
const NamePicker: React.FC<Props> = ({
  label,
  required,
  options,
  value,
  mode,
  onChange,
  emptyMessage,
  emptyPlaceholder,
  newPlaceholder,
  defaultOption,
  createLabel = "-- Create new --",
}) => {
  if (options.length === 0 && defaultOption === undefined) {
    return (
      <div>
        <label className="block text-sm font-medium text-foreground mb-2">
          {label} {required && <span className="text-destructive">*</span>}
        </label>
        <p className="text-xs text-muted-foreground mb-2">{emptyMessage}</p>
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value, "new")}
          placeholder={emptyPlaceholder ?? newPlaceholder}
          className={textInput}
        />
      </div>
    );
  }

  return (
    <div>
      <label className="block text-sm font-medium text-foreground mb-2">
        {label} {required && <span className="text-destructive">*</span>}
      </label>
      <select
        value={mode === "new" ? "__new__" : value}
        onChange={(e) => {
          if (e.target.value === "__new__") onChange("", "new");
          else onChange(e.target.value, "existing");
        }}
        className={selectInput}
      >
        {defaultOption !== undefined ? (
          <option value={defaultOption}>{defaultOption}</option>
        ) : (
          <option value="" disabled>Select…</option>
        )}
        {options.filter((o) => o !== defaultOption).map((name) => (
          <option key={name} value={name}>{name}</option>
        ))}
        <option value="__new__">{createLabel}</option>
      </select>
      {mode === "new" && (
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value, "new")}
          placeholder={newPlaceholder}
          className={`mt-2 ${textInput}`}
        />
      )}
    </div>
  );
};

export default NamePicker;
