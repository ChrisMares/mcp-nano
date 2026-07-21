import React from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

interface ExpandableCardProps {
  title: string;
  subtitle?: string;
  expanded: boolean;
  onToggle: () => void;
  badge?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
}

const ExpandableCard: React.FC<ExpandableCardProps> = ({
  title,
  subtitle,
  expanded,
  onToggle,
  badge,
  actions,
  children,
}) => (
  <div className="border border-border rounded-lg bg-background overflow-hidden">
    <div className="flex items-center justify-between hover:bg-muted/30 transition-colors">
      {/* Clickable toggle area */}
      <button
        type="button"
        onClick={onToggle}
        className="flex-1 flex items-center gap-3 p-4 text-left min-w-0"
      >
        {expanded ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
        <div className="min-w-0">
          <p className="font-medium text-foreground truncate">{title}</p>
          {subtitle && <p className="text-sm text-muted-foreground truncate">{subtitle}</p>}
        </div>
        {badge}
      </button>
      {/* Actions sit outside the toggle button */}
      {actions && (
        <div className="flex items-center gap-2 px-4">
          {actions}
        </div>
      )}
    </div>
    {expanded && <div className="border-t border-border p-4">{children}</div>}
  </div>
);

export default ExpandableCard;
