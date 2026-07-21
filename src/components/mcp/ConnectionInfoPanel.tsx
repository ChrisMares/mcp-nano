import React, { useState } from "react";
import { Copy, Check } from "lucide-react";
import type { ConnectionInfo } from "@/types/mcp";
import { fieldLabel, btnCopy, codeBlock } from "@/styles/classes";

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

  const copyToClipboard = async (text: string, field: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopiedField(field);
    setTimeout(() => setCopiedField(null), 2000);
  };

  return (
    <div className="space-y-4">
      <div>
        <label className={fieldLabel}>Server URL</label>
        <div className="flex items-center gap-2">
          <code className={`flex-1 ${codeBlock}`}>{info.full_url}</code>
          <CopyButton field="url" text={info.full_url} copiedField={copiedField} onCopy={copyToClipboard} />
        </div>
      </div>

      <div>
        <div className="flex items-center justify-between mb-1">
          <label className={fieldLabel}>Claude Desktop Config</label>
          <CopyButton field="claude_desktop" text={JSON.stringify(info.config_snippets.claude_desktop, null, 2)} copiedField={copiedField} onCopy={copyToClipboard} />
        </div>
        <pre className={`${codeBlock} text-xs overflow-x-auto`}>
          {JSON.stringify(info.config_snippets.claude_desktop, null, 2)}
        </pre>
      </div>

      <div>
        <div className="flex items-center justify-between mb-1">
          <label className={fieldLabel}>OpenCode Config</label>
          <CopyButton field="opencode" text={JSON.stringify(info.config_snippets.opencode, null, 2)} copiedField={copiedField} onCopy={copyToClipboard} />
        </div>
        <pre className={`${codeBlock} text-xs overflow-x-auto`}>
          {JSON.stringify(info.config_snippets.opencode, null, 2)}
        </pre>
      </div>

      <div>
        <div className="flex items-center justify-between mb-1">
          <label className={fieldLabel}>VS Code Config</label>
          <CopyButton field="vscode" text={JSON.stringify(info.config_snippets.vscode, null, 2)} copiedField={copiedField} onCopy={copyToClipboard} />
        </div>
        <pre className={`${codeBlock} text-xs overflow-x-auto`}>
          {JSON.stringify(info.config_snippets.vscode, null, 2)}
        </pre>
      </div>
    </div>
  );
};

export default ConnectionInfoPanel;
