import React from "react";
import { CheckCircle } from "lucide-react";
import {
  wizardStepDot, wizardStepDotActive, wizardStepDotCompleted, wizardStepDotPending,
  wizardStepLabel, wizardConnector,
} from "@/styles/classes";

interface StepIndicatorProps {
  current: number;
  total: number;
  labels: string[];
  onStepClick: (step: number) => void;
}

const StepIndicator: React.FC<StepIndicatorProps> = ({ current, total, labels, onStepClick }) => (
  <div className="flex items-center justify-center mb-8">
    {Array.from({ length: total }, (_, i) => {
      const stepNum = i + 1;
      const isCompleted = stepNum < current;
      const isActive = stepNum === current;
      const canClick = isCompleted;
      return (
        <React.Fragment key={i}>
          <div className="flex flex-col items-center min-w-[70px]">
            <button
              type="button"
              disabled={!canClick}
              onClick={() => canClick && onStepClick(stepNum)}
              className={`${wizardStepDot} ${isCompleted ? wizardStepDotCompleted : isActive ? wizardStepDotActive : wizardStepDotPending} ${canClick ? "cursor-pointer hover:scale-110 transition-transform" : "cursor-default"}`}
            >
              {isCompleted ? <CheckCircle size={16} /> : stepNum}
            </button>
            <span className={`${wizardStepLabel} ${isActive ? "text-foreground" : "text-muted-foreground"}`}>
              {labels[i]}
            </span>
          </div>
          {i < total - 1 && (
            <div className={`${wizardConnector} ${stepNum < current ? "bg-success" : "bg-border"} self-start mt-4`} />
          )}
        </React.Fragment>
      );
    })}
  </div>
);

export default StepIndicator;
