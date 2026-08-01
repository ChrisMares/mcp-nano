import React, { useRef, useState } from "react";
import { Copy, Check, ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import { JsonView, collapseAllNested, allExpanded, darkStyles, defaultStyles } from "react-json-view-lite";
import "react-json-view-lite/dist/index.css";
import { btnCopy } from "@/styles/classes";
import { copyTextToClipboard } from "@/lib/clipboard";
import type { RagResponse } from "@/types/rag";

interface Props {
  data?: RagResponse;
  theme: "light" | "dark";
}

const RagResultsPanel: React.FC<Props> = ({ data, theme }) => {
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const copyTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleCopy = async () => {
    await copyTextToClipboard(JSON.stringify(data, null, 2));
    setCopied(true);
    if (copyTimeout.current) clearTimeout(copyTimeout.current);
    copyTimeout.current = setTimeout(() => setCopied(false), 2000);
  };

  const handleToggleExpand = () => {
    const next = !expanded;
    setExpanded(next);
    if (next) setTimeout(() => scrollRef.current?.scrollIntoView({ behavior: "smooth", block: "end" }), 50);
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <h2 className="text-lg font-semibold text-foreground">Results</h2>
        {data && (
          <div className="flex items-center gap-2">
            <button onClick={handleToggleExpand} className={btnCopy} title={expanded ? "Collapse all" : "Expand all"}>
              {expanded ? <ChevronsDownUp size={14} /> : <ChevronsUpDown size={14} />}
              {expanded ? "Collapse" : "Expand All"}
            </button>
            <button onClick={handleCopy} className={btnCopy} title="Copy to clipboard">
              {copied ? <Check size={14} className="text-success" /> : <Copy size={14} />}
              {copied ? "Copied" : "Copy"}
            </button>
          </div>
        )}
      </div>
      <div ref={scrollRef} className="border border-border rounded-lg bg-muted/30 max-h-[500px] overflow-y-auto">
        {data ? (
          <div className="p-4 text-sm font-mono">
            <JsonView
              data={data}
              shouldExpandNode={expanded ? allExpanded : collapseAllNested}
              style={theme === "dark" ? darkStyles : defaultStyles}
            />
          </div>
        ) : (
          <p className="p-6 text-muted-foreground text-center text-sm">
            Enter a query above to fetch relevant context from your embedded data.
          </p>
        )}
      </div>
    </div>
  );
};

export default RagResultsPanel;
