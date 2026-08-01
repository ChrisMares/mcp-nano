import React, { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { getFiles, getMcpServers, getMcpServer, getWebsites } from "@/utils/apicalls";
import { Database, Search, Plug, Code2, FileText, Server, Wrench, Loader2, ArrowRight } from "lucide-react";
import PageHead from "@/components/shared/PageHead";
import { card, cardInner, btnPrimary } from "@/styles/classes";
import type { DashboardStats } from "@/types/dashboard";
import { emptyDashboardStats } from "@/types/dashboard";

const steps = [
  {
    num: 1,
    icon: Database,
    title: "Embed Your Data",
    description:
      "Upload documents or code repositories. Files are indexed as embeddings for search and MCP tools.",
    link: "/embed/upload",
    cta: "Start Embedding",
    color: "text-brand-gold-bright",
    badgeBg: "bg-primary text-primary-foreground",
  },
  {
    num: 2,
    icon: Search,
    title: "Fetch Context",
    description:
      "Run natural-language queries against embedded data and review the returned context.",
    link: "/query/fetch",
    cta: "Try a Search",
    color: "text-brand-cyan",
    badgeBg: "bg-brand-cyan text-brand-surface",
  },
  {
    num: 3,
    icon: Plug,
    title: "Create MCP Server & Tools",
    description:
      "Create MCP servers and tools, then scope each tool to selected repos, document groups, or websites.",
    link: "/mcp/create",
    cta: "Create a Server",
    color: "text-brand-gold-bright",
    badgeBg: "bg-primary text-primary-foreground",
  },
];

const statCards = [
  { key: "repos" as const, label: "Repositories", icon: Code2, link: "/embed/data" },
  { key: "documents" as const, label: "Documents", icon: FileText, link: "/embed/data" },
  { key: "servers" as const, label: "MCP Servers", icon: Server, link: "/mcp/manage" },
  { key: "tools" as const, label: "MCP Tools", icon: Wrench, link: "/mcp/manage" },
];

const Dashboard: React.FC = () => {
  const [stats, setStats] = useState<DashboardStats>(emptyDashboardStats);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      getFiles().catch(() => ({ repos: [], documents: [] })),
      getMcpServers().catch(() => ({ servers: [] })),
      getWebsites().catch(() => ({ websites: [] })),
    ]).then(async ([filesRes, serversRes, websitesRes]) => {
      const servers = serversRes.servers || [];
      const serverDetails = await Promise.all(
        servers.map((s) => getMcpServer(s.id).catch(() => null))
      );
      const toolCount = serverDetails.reduce((sum, item) => {
        if (!item) return sum;
        return sum + (item.server?.tools?.length ?? 0);
      }, 0);
      const websiteCount = websitesRes.websites?.length ?? 0;

      setStats({
        repos: filesRes.repos?.length ?? 0,
        documents: (filesRes.documents?.length ?? 0) + websiteCount,
        servers: servers.length,
        tools: toolCount,
      });
    }).finally(() => setLoading(false));
  }, []);

  const hasData = stats.repos + stats.documents > 0;
  const disabledHint = "Embed your data first -- there's nothing to search or serve yet.";

  return (
    <div className="space-y-6">
      <PageHead
        title="Dashboard"
        description="Dashboard for embedding data, searching context, and managing MCP tools."
      />

      {/* Welcome header */}
      <div className={card}>
        <h1 className="text-2xl font-bold text-foreground mb-1">Welcome to NASA MCP</h1>
      </div>

      {/* Getting started steps */}
      <div>
        <h2 className="text-lg font-semibold text-foreground mb-4">Getting Started</h2>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {steps.map((step) => {
            const disabled = !loading && !hasData && step.num > 1;
            return (
              <div key={step.num} className={`${card} flex flex-col`}>
                <div className="flex items-center gap-3 mb-3">
                  <span className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold ${step.badgeBg}`}>
                    {step.num}
                  </span>
                  <step.icon className={`w-5 h-5 ${step.color}`} />
                  <h3 className="font-semibold text-foreground">{step.title}</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed flex-1 mb-4">
                  {step.description}
                </p>
                {disabled ? (
                  <div>
                    <p className="text-xs italic text-muted-foreground mb-2 text-center font-serif">Embed data first</p>
                    <span
                      title={disabledHint}
                      className={`${btnPrimary} justify-center text-sm opacity-40 cursor-not-allowed`}
                    >
                      {step.cta}
                      <ArrowRight className="w-4 h-4" />
                    </span>
                  </div>
                ) : (
                  <Link to={step.link} className={`${btnPrimary} justify-center text-sm`}>
                    {step.cta}
                    <ArrowRight className="w-4 h-4" />
                  </Link>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* Stats overview */}
      <div>
        <h2 className="text-lg font-semibold text-foreground mb-4">Overview</h2>
        {loading ? (
          <div className={card}>
            <div className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading stats...
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-3">
            {statCards.map((sc) => (
              <Link key={sc.key} to={sc.link} className={`${cardInner} hover:border-primary/40 cursor-pointer transition-colors`}>
                <div className="flex items-center gap-2 mb-2">
                  <sc.icon className="w-4 h-4 text-muted-foreground" />
                  <span className="text-xs text-muted-foreground">{sc.label}</span>
                </div>
                <span className="text-2xl font-bold text-foreground">{stats[sc.key]}</span>
              </Link>
            ))}

          </div>
        )}
      </div>
    </div>
  );
};

export default Dashboard;
