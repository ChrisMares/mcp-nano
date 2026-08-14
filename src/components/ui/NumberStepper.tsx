import React from "react";
import { ChevronUp, ChevronDown } from "lucide-react";
import { fieldLabel } from "@/styles/classes";

interface NumberStepperProps {
  label?: string;
  hint?: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
  size?: "md" | "sm";
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

const NumberStepper: React.FC<NumberStepperProps> = ({
  label,
  hint,
  value,
  onChange,
  min = 1,
  max = 50,
  step = 1,
  disabled = false,
  size = "md",
}) => {
  const canDecrement = !disabled && value > min;
  const canIncrement = !disabled && value < max;
  const isSm = size === "sm";

  const commit = (next: number) => onChange(clamp(next, min, max));

  const handleInputChange = (raw: string) => {
    if (raw === "") return;
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    commit(parsed);
  };

  return (
    <div>
      {label && <label className={fieldLabel}>{label}</label>}
      <div className={`flex items-stretch ${isSm ? "w-16" : "w-32"}`}>
        <input
          type="number"
          inputMode="numeric"
          value={value}
          min={min}
          max={max}
          step={step}
          disabled={disabled}
          onChange={(e) => handleInputChange(e.target.value)}
          onBlur={(e) => commit(Number.parseInt(e.target.value, 10) || min)}
          className={`w-full border border-border rounded-l-md bg-input text-foreground shadow-sm focus:outline-none focus:ring-2 focus:ring-brand-cyan/50 focus:border-brand-cyan transition-colors [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none disabled:opacity-50 ${isSm ? "px-1.5 py-1 text-xs" : "px-3 py-2"}`}
        />
        <div className="flex flex-col border border-l-0 border-border rounded-r-md overflow-hidden">
          <button
            type="button"
            aria-label="Increase value"
            disabled={!canIncrement}
            onClick={() => commit(value + step)}
            className={`flex-1 flex items-center justify-center bg-muted/50 hover:bg-muted disabled:opacity-40 disabled:cursor-not-allowed transition-colors border-b border-border ${isSm ? "px-1" : "px-2"}`}
          >
            <ChevronUp size={isSm ? 10 : 14} />
          </button>
          <button
            type="button"
            aria-label="Decrease value"
            disabled={!canDecrement}
            onClick={() => commit(value - step)}
            className={`flex-1 flex items-center justify-center bg-muted/50 hover:bg-muted disabled:opacity-40 disabled:cursor-not-allowed transition-colors ${isSm ? "px-1" : "px-2"}`}
          >
            <ChevronDown size={isSm ? 10 : 14} />
          </button>
        </div>
      </div>
      {hint && <p className="text-xs text-muted-foreground mt-1">{hint}</p>}
    </div>
  );
};

export default NumberStepper;
