import React, { useState, useMemo, useRef, useEffect } from "react";

interface DataSelectListProps {
  title: string;
  options: string[];
  selected: Set<string>;
  onToggle: (item: string) => void;
  onSetSelected: (next: Set<string>) => void;
}

const DataSelectList: React.FC<DataSelectListProps> = ({
  title,
  options,
  selected,
  onToggle,
  onSetSelected,
}) => {
  const [filter, setFilter] = useState("");
  const checkAllRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    if (!filter.trim()) return options;
    const lower = filter.toLowerCase();
    return options.filter((o) => o.toLowerCase().includes(lower));
  }, [options, filter]);

  const filteredSelectedCount = filtered.filter((o) => selected.has(o)).length;
  const allFilteredSelected = filtered.length > 0 && filteredSelectedCount === filtered.length;
  const someFilteredSelected = filteredSelectedCount > 0 && !allFilteredSelected;

  useEffect(() => {
    if (checkAllRef.current) checkAllRef.current.indeterminate = someFilteredSelected;
  }, [someFilteredSelected]);

  if (options.length === 0) return null;

  const handleToggleAll = () => {
    const next = new Set(selected);
    if (allFilteredSelected) {
      filtered.forEach((o) => next.delete(o));
    } else {
      filtered.forEach((o) => next.add(o));
    }
    onSetSelected(next);
  };

  return (
    <div>
      <h3 className="text-sm font-semibold text-foreground mb-1.5">{title}</h3>

      <div className="flex items-center gap-2 border border-border border-b-0 rounded-t-md bg-muted/40 px-2 py-1.5">
        <label className="flex items-center gap-2 cursor-pointer" title={allFilteredSelected ? "Deselect All" : "Select All"}>
          <input
            ref={checkAllRef}
            type="checkbox"
            checked={allFilteredSelected}
            onChange={handleToggleAll}
            className="w-3.5 h-3.5 rounded border-border text-primary focus:ring-primary"
          />
          <span className="text-xs text-muted-foreground whitespace-nowrap">
            {filteredSelectedCount}/{filtered.length}
          </span>
        </label>
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Search…"
          className="flex-1 min-w-0 text-sm bg-transparent border-none outline-none placeholder:text-muted-foreground text-foreground"
        />
      </div>

      <div className="border border-border rounded-b-md bg-background h-48 overflow-y-auto p-1.5">
        {filtered.length === 0 ? (
          <p className="text-xs text-muted-foreground italic px-2 py-1">No matches</p>
        ) : (
          filtered.map((opt) => (
            <label key={opt} className="flex items-center gap-2 cursor-pointer px-2 py-1 rounded hover:bg-muted/50">
              <input
                type="checkbox"
                checked={selected.has(opt)}
                onChange={() => onToggle(opt)}
                className="w-3.5 h-3.5 rounded border-border text-primary focus:ring-primary"
              />
              <span className="text-sm text-foreground truncate">{opt}</span>
            </label>
          ))
        )}
      </div>
    </div>
  );
};

export default DataSelectList;
