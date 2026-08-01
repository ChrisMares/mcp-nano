import React, { useState } from "react";
import { Copy, Check } from "lucide-react";
import type { ConnectionInfo } from "@/types/mcp";
import { fieldLabel, btnCopy, codeBlock } from "@/styles/classes";
import { copyTextToClipboard } from "@/lib/clipboard";

interface ConnectionInfoPanelProps {
  info: ConnectionInfo;
}

interface CopyButtonProps {
  field: string;
  text: string;
  copiedField: string | null;
  onCopy: (text: string, field: string) => void;
}

const CopyButton: React.FC<CopyButtonProps> = ({ field, text, copiedField, onCopy }) => (
  <button onClick={() => onCopy(text, field)} className={btnCopy}>
    {copiedField === field ? <Check size={14} className="text-success" /> : <Copy size={14} />}
    {copiedField === field ? "Copied" : "Copy"}
  </button>
);

const ConnectionInfoPanel: React.FC<ConnectionInfoPanelProps> = ({ info }) => {
  const [copiedField, setCopiedField] = useState<string | null>(null);

  const handleCopy = async (text: string, field: string) => {
    await copyTextToClipboard(text);
    setCopiedField(field);
    setTimeout(() => setCopiedField(null), 2000);
  };

  const snippetSections = [
    { field: "claude_desktop", label: "Claude Desktop Config", snippet: info.config_snippets.claude_desktop },
    { field: "opencode", label: "OpenCode Config", snippet: info.config_snippets.opencode },
    { field: "vscode", label: "VS Code Config", snippet: info.config_snippets.vscode },
  ];

  return (
    <div className="space-y-4">
      <div>
        <label className={fieldLabel}>Server URL</label>
        <div className="flex items-center gap-2">
          <code className={`flex-1 ${codeBlock}`}>{info.full_url}</code>
          <CopyButton field="url" text={info.full_url} copiedField={copiedField} onCopy={handleCopy} />
        </div>
      </div>

      {snippetSections.map(({ field, label, snippet }) => {
        const text = JSON.stringify(snippet, null, 2);
        return (
          <div key={field}>
            <div className="flex items-center justify-between mb-1">
              <label className={fieldLabel}>{label}</label>
              <CopyButton field={field} text={text} copiedField={copiedField} onCopy={handleCopy} />
            </div>
            <pre className={`${codeBlock} text-xs overflow-x-auto`}>{text}</pre>
          </div>
        );
      })}
    </div>
  );
};

export default ConnectionInfoPanel;
