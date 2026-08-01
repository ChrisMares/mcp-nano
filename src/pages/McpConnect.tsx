import React, { useState, useEffect, useCallback } from "react";
import { getMcpServers, getMcpConnectionInfo } from "@/utils/apicalls";
import { Loader2 } from "lucide-react";
import ExpandableCard from "@/components/mcp/ExpandableCard";
import PageHead from "@/components/shared/PageHead";
import ConnectionInfoPanel from "@/components/mcp/ConnectionInfoPanel";
import type { McpServer, ConnectionInfo } from "@/types/mcp";
import { card, alertError, loader } from "@/styles/classes";

const McpConnect: React.FC = () => {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [expandedServerId, setExpandedServerId] = useState<string | null>(null);
  const [connectionInfoMap, setConnectionInfoMap] = useState<Record<string, ConnectionInfo>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getMcpServers()
      .then((res) => setServers(res?.servers || []))
      .catch(() => setError("Failed to load servers"))
      .finally(() => setLoading(false));
  }, []);

  // Auto-expand and fetch connection info when there's exactly one server
  useEffect(() => {
    if (loading || servers.length !== 1) return;
    const server = servers[0];
    setExpandedServerId(server.id);
    if (!connectionInfoMap[server.id]) {
      getMcpConnectionInfo(server.id)
        .then((res) => { if (res) setConnectionInfoMap((prev) => ({ ...prev, [server.id]: res })); })
        .catch(() => setError("Failed to load connection info"));
    }
  }, [loading, servers, connectionInfoMap]);

  const toggleServer = useCallback(async (serverId: string) => {
    if (expandedServerId === serverId) {
      setExpandedServerId(null);
      return;
    }
    setExpandedServerId(serverId);

    // Fetch connection info if not cached
    if (!connectionInfoMap[serverId]) {
      try {
        const res = await getMcpConnectionInfo(serverId);
        if (res) {
          setConnectionInfoMap((prev) => ({ ...prev, [serverId]: res }));
        }
      } catch {
        setError("Failed to load connection info");
      }
    }
  }, [expandedServerId, connectionInfoMap]);

  return (
    <div className={card}>
      <PageHead
        title="Connect MCP Server"
        description="View MCP server connection URLs and client configuration snippets."
      />
      <h1 className="text-2xl font-bold text-foreground mb-2">MCP Connection</h1>
      <p className="text-muted-foreground mb-6">
        Expand a server to see its connection URL and config snippets for your MCP client.
      </p>

      {error && <div className={alertError}>{error}</div>}

      {loading ? (
        <div className={loader}>
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading servers...
        </div>
      ) : servers.length === 0 ? (
        <p className="text-muted-foreground">No servers found. Create one on the Create page first.</p>
      ) : (
        <div className="max-w-3xl space-y-3">
          {servers.map((server) => (
            <ExpandableCard
              key={server.id}
              title={server.name}
              subtitle={server.description ?? undefined}
              expanded={expandedServerId === server.id}
              onToggle={() => toggleServer(server.id)}
            >
              {connectionInfoMap[server.id] ? (
                <ConnectionInfoPanel info={connectionInfoMap[server.id]} />
              ) : (
                <div className={loader}>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Loading connection info...
                </div>
              )}
            </ExpandableCard>
          ))}
        </div>
      )}
    </div>
  );
};

export default McpConnect;
