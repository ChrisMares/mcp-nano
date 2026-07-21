import React from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { UrlTreeNode } from "@/types/embed";

interface PathTreeListProps {
  nodes: UrlTreeNode[];
  checked: Set<string>;
  collapsedPaths: Set<string>;
  branchLeaves: Map<string, string[]>;
  onToggleUrl: (url: string) => void;
  onToggleUrlSet: (urls: string[]) => void;
  onToggleCollapse: (path: string) => void;
}

const PathTreeList: React.FC<PathTreeListProps> = React.memo(({
  nodes, checked, collapsedPaths, branchLeaves,
  onToggleUrl, onToggleUrlSet, onToggleCollapse,
}) => {
  return (
    <>
      {nodes.map((node) => {
        const isBranch = node.children.length > 0;
        const leafUrls = branchLeaves.get(node.fullPath) ?? (node.url ? [node.url] : []);
        const checkedCount = leafUrls.filter(u => checked.has(u)).length;
        const allChecked = leafUrls.length > 0 && checkedCount === leafUrls.length;
        const isCollapsed = collapsedPaths.has(node.fullPath);
        const label = node.segment || "/";

        if (!isBranch && node.url) {
          return (
            <label key={node.url} className="flex items-start gap-2 py-1 cursor-pointer hover:bg-muted/50 rounded px-1">
              <input
                type="checkbox"
                checked={checked.has(node.url)}
                onChange={() => onToggleUrl(node.url!)}
                className="w-4 h-4 rounded accent-primary cursor-pointer mt-0.5 shrink-0"
              />
              <span className="text-xs text-foreground break-all leading-relaxed">{label}</span>
            </label>
          );
        }

        return (
          <div key={node.fullPath}>
            <div className="flex items-center gap-2 py-1.5 hover:bg-muted/30 rounded px-1">
              <button
                onClick={() => onToggleCollapse(node.fullPath)}
                className="text-muted-foreground hover:text-foreground shrink-0"
              >
                {isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
              </button>
              <input
                type="checkbox"
                checked={allChecked}
                ref={el => { if (el) el.indeterminate = checkedCount > 0 && checkedCount < leafUrls.length; }}
                onChange={() => onToggleUrlSet(leafUrls)}
                className="w-4 h-4 rounded accent-primary cursor-pointer shrink-0"
              />
              <span className="text-xs font-medium text-foreground truncate">{label}</span>
              <span className="text-xs text-muted-foreground ml-auto shrink-0">
                {checkedCount}/{leafUrls.length}
              </span>
            </div>

            {!isCollapsed && (
              <div className="ml-6 space-y-0.5 border-l border-border/50 pl-3">
                {node.url && (
                  <label className="flex items-start gap-2 py-1 cursor-pointer hover:bg-muted/50 rounded px-1">
                    <input
                      type="checkbox"
                      checked={checked.has(node.url)}
                      onChange={() => onToggleUrl(node.url!)}
                      className="w-4 h-4 rounded accent-primary cursor-pointer mt-0.5 shrink-0"
                    />
                    <span className="text-xs text-muted-foreground break-all leading-relaxed">{label} (this page)</span>
                  </label>
                )}
                <PathTreeList
                  nodes={node.children}
                  checked={checked}
                  collapsedPaths={collapsedPaths}
                  branchLeaves={branchLeaves}
                  onToggleUrl={onToggleUrl}
                  onToggleUrlSet={onToggleUrlSet}
                  onToggleCollapse={onToggleCollapse}
                />
              </div>
            )}
          </div>
        );
      })}
    </>
  );
});

export default PathTreeList;
