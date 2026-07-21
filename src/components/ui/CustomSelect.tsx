import React, { useState, useRef, useEffect, useCallback } from "react";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  style?: React.CSSProperties;
}

interface CustomSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  className?: string;
  placeholder?: string;
}

// Reusable dropdown select with cyan-light hover styling
const CustomSelect: React.FC<CustomSelectProps> = ({
  value,
  onChange,
  options,
  className = "",
  placeholder,
}) => {
  const [open, setOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const selectedOption = options.find((o) => o.value === value);
  const displayLabel = selectedOption?.label ?? placeholder ?? "";

  // Close on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  // Scroll highlighted item into view
  useEffect(() => {
    if (!open || highlightedIndex < 0 || !listRef.current) return;
    const item = listRef.current.children[highlightedIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: "nearest" });
  }, [highlightedIndex, open]);

  const toggle = useCallback(() => {
    setOpen((prev) => {
      if (!prev) {
        const idx = options.findIndex((o) => o.value === value && !o.disabled);
        setHighlightedIndex(idx >= 0 ? idx : 0);
      }
      return !prev;
    });
  }, [options, value]);

  const select = useCallback(
    (opt: SelectOption) => {
      if (opt.disabled) return;
      onChange(opt.value);
      setOpen(false);
    },
    [onChange],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!open) {
        if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
          e.preventDefault();
          toggle();
        }
        return;
      }

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setHighlightedIndex((i) => {
            let next = i + 1;
            while (next < options.length && options[next].disabled) next++;
            return next < options.length ? next : i;
          });
          break;
        case "ArrowUp":
          e.preventDefault();
          setHighlightedIndex((i) => {
            let next = i - 1;
            while (next >= 0 && options[next].disabled) next--;
            return next >= 0 ? next : i;
          });
          break;
        case "Enter":
        case " ":
          e.preventDefault();
          if (highlightedIndex >= 0 && highlightedIndex < options.length) {
            select(options[highlightedIndex]);
          }
          break;
        case "Escape":
        case "Tab":
          setOpen(false);
          break;
      }
    },
    [open, highlightedIndex, options, toggle, select],
  );

  return (
    <div ref={containerRef} className={`custom-select-root ${className}`}>
      {/* Trigger button */}
      <button
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={toggle}
        onKeyDown={handleKeyDown}
        className="custom-select-trigger"
      >
        <span className={selectedOption ? "" : "custom-select-placeholder"}>
          {displayLabel}
        </span>
        <svg className="custom-select-chevron" viewBox="0 0 20 20" fill="none" stroke="currentColor">
          <path d="m6 8 4 4 4-4" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>

      {/* Dropdown list */}
      {open && (
        <ul
          ref={listRef}
          role="listbox"
          className="custom-select-list"
        >
          {options.map((opt, i) => (
            <li
              key={opt.value}
              role="option"
              aria-selected={opt.value === value}
              aria-disabled={opt.disabled}
              className={[
                "custom-select-option",
                opt.value === value ? "custom-select-option-selected" : "",
                i === highlightedIndex ? "custom-select-option-highlighted" : "",
                opt.disabled ? "custom-select-option-disabled" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onMouseEnter={() => !opt.disabled && setHighlightedIndex(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                select(opt);
              }}
            >
              {opt.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};

export default CustomSelect;
