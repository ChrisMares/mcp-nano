import React, { useState, useMemo, useEffect, useCallback } from "react";
import { Globe, RefreshCw, AlertTriangle, CheckCircle, Loader2, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import { btnPrimary, btnSecondary } from "@/styles/classes";
import type { EmbedJob, UrlTreeNode } from "@/types/embed";
import PathTreeList from "@/components/uploadfiles/PathTreeList";

interface UrlGroup {
  hostname: string;
  isBaseDomain: boolean;
  urls: string[];
}

interface Props {
  websiteUrl: string;
  crawledUrls: string[];
  crawlError: boolean;
  groupName: string;
  embedJobId: string | null;
  activeJobs: EmbedJob[];
  completedJobs: EmbedJob[];
  onEmbed: (urls: string[]) => void;
  onStartAnother: () => void;
}

function sortUrls(urls: string[]): string[] {
  return [...urls].sort((a, b) => {
    const pa = new URL(a).pathname.replace(/\/$/, "");
    const pb = new URL(b).pathname.replace(/\/$/, "");
    const da = pa.split("/").filter(Boolean).length;
    const db = pb.split("/").filter(Boolean).length;
    if (da !== db) return da - db;
    return a.localeCompare(b);
  });
}

function buildUrlGroups(urls: string[], baseUrl: string): UrlGroup[] {
  if (urls.length === 0) return [];
  const baseHostname = new URL(baseUrl).hostname;

  const groups = new Map<string, string[]>();
  for (const url of urls) {
    const hostname = new URL(url).hostname;
    if (!groups.has(hostname)) groups.set(hostname, []);
    groups.get(hostname)!.push(url);
  }

  const result: UrlGroup[] = [];

  if (groups.has(baseHostname)) {
    result.push({ hostname: baseHostname, isBaseDomain: true, urls: sortUrls(groups.get(baseHostname)!) });
    groups.delete(baseHostname);
  }

  for (const hostname of [...groups.keys()].sort()) {
    result.push({ hostname, isBaseDomain: false, urls: sortUrls(groups.get(hostname)!) });
  }

  return result;
}

function buildPathTree(urls: string[]): UrlTreeNode[] {
  const root: UrlTreeNode = { segment: "", fullPath: "/", depth: 0, children: [], url: null };

  for (const url of urls) {
    const pathname = new URL(url).pathname.replace(/\/$/, "") || "/";
    const segments = pathname === "/" ? [] : pathname.slice(1).split("/");

    let node = root;
    for (let i = 0; i < segments.length; i++) {
      let child = node.children.find(c => c.segment === segments[i]);
      if (!child) {
        const childPath = "/" + segments.slice(0, i + 1).join("/");
        child = { segment: segments[i], fullPath: childPath, depth: node.depth + 1, children: [], url: null };
        node.children.push(child);
      }
      node = child;
    }
    node.url = url;
  }

  return root.children;
}

function collectLeafUrls(nodes: UrlTreeNode[]): string[] {
  const urls: string[] = [];
  for (const node of nodes) {
    if (node.url) urls.push(node.url);
    if (node.children.length > 0) urls.push(...collectLeafUrls(node.children));
  }
  return urls;
}

function collectBranchPaths(nodes: UrlTreeNode[]): string[] {
  const paths: string[] = [];
  for (const node of nodes) {
    if (node.children.length > 0) {
      paths.push(node.fullPath);
      paths.push(...collectBranchPaths(node.children));
    }
  }
  return paths;
}

function findNode(nodes: UrlTreeNode[], fullPath: string): UrlTreeNode | null {
  for (const node of nodes) {
    if (node.fullPath === fullPath) return node;
    if (node.children.length > 0) {
      const found = findNode(node.children, fullPath);
      if (found) return found;
    }
  }
  return null;
}

const WebsiteProcessingStep: React.FC<Props> = ({
  websiteUrl, crawledUrls, crawlError, groupName,
  embedJobId, activeJobs, completedJobs,
  onEmbed, onStartAnother,
}) => {
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [collapsedHosts, setCollapsedHosts] = useState<Set<string>>(new Set());
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());

  const urlGroups = useMemo(() => buildUrlGroups(crawledUrls, websiteUrl), [crawledUrls, websiteUrl]);

  const { pathTrees, branchLeaves, allBranchPaths } = useMemo(() => {
    const treeMap = new Map<string, UrlTreeNode[]>();
    for (const group of urlGroups) {
      treeMap.set(group.hostname, buildPathTree(group.urls));
    }

    const leafMap = new Map<string, string[]>();
    const branchPaths: string[] = [];
    for (const [, nodes] of treeMap) {
      for (const bp of collectBranchPaths(nodes)) {
        branchPaths.push(bp);
        const node = findNode(nodes, bp);
        if (node) leafMap.set(bp, collectLeafUrls(node.children));
      }
    }

    return { pathTrees: treeMap, branchLeaves: leafMap, allBranchPaths: branchPaths };
  }, [urlGroups]);

  useEffect(() => {
    setChecked(new Set(crawledUrls));
  }, [crawledUrls]);

  useEffect(() => {
    if (urlGroups.length > 0) {
      const base = urlGroups[0].hostname;
      setCollapsedHosts(new Set(urlGroups.filter(g => g.hostname !== base).map(g => g.hostname)));
    }
    setCollapsedPaths(new Set(allBranchPaths.filter(p => {
      const parts = p.split("/").filter(Boolean);
      return parts.length >= 2;
    })));
  }, [urlGroups, allBranchPaths]);

  const totalChecked = checked.size;

  const toggleUrl = useCallback((url: string) => {
    setChecked(prev => {
      const next = new Set(prev);
      if (next.has(url)) next.delete(url); else next.add(url);
      return next;
    });
  }, []);

  const toggleUrlSet = useCallback((urls: string[]) => {
    const allChecked = urls.every(u => checked.has(u));
    setChecked(prev => {
      const next = new Set(prev);
      for (const url of urls) {
        if (allChecked) next.delete(url); else next.add(url);
      }
      return next;
    });
  }, [checked]);

  const toggleAll = useCallback(() => {
    const allChecked = checked.size === crawledUrls.length;
    setChecked(allChecked ? new Set() : new Set(crawledUrls));
  }, [checked, crawledUrls]);

  const toggleHostCollapse = useCallback((hostname: string) => {
    setCollapsedHosts(prev => {
      const next = new Set(prev);
      if (next.has(hostname)) next.delete(hostname);
      else next.add(hostname);
      return next;
    });
  }, []);

  const togglePathCollapse = useCallback((path: string) => {
    setCollapsedPaths(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const expandAllPaths = useCallback(() => setCollapsedPaths(new Set()), []);
  const collapseAllPaths = useCallback(() => setCollapsedPaths(new Set(allBranchPaths)), [allBranchPaths]);

  const embedJob = useMemo(() => {
    if (!embedJobId) return null;
    return activeJobs.find(j => j.job_id === embedJobId) ?? completedJobs.find(j => j.job_id === embedJobId) ?? null;
  }, [embedJobId, activeJobs, completedJobs]);

  const isEmbedding = !!embedJobId && !!embedJob && embedJob.progress_percentage < 100;
  const isEmbedDone = !!embedJobId && !!embedJob && embedJob.progress_percentage >= 100 && embedJob.status === "COMPLETED";

  if (!embedJobId) {
    return (
      <div>
        <div className="flex items-center gap-3 mb-4">
          <div className="p-2 rounded-lg bg-primary/15">
            <Globe size={22} className="text-primary" />
          </div>
          <h2 className="text-lg font-semibold text-foreground">Review Crawl Results</h2>
        </div>

        {crawlError && (
          <div className="flex items-start gap-2 mb-4 px-3 py-2.5 rounded-lg bg-warning/10 border border-warning/40 text-sm text-warning">
            <AlertTriangle size={16} className="shrink-0 mt-0.5" />
            <span>Crawl failed. Please check the URL and try again.</span>
          </div>
        )}

        {crawledUrls.length === 0 && !crawlError && (
          <div className="flex flex-col items-center gap-3 py-8 text-muted-foreground">
            <Loader2 size={28} className="animate-spin text-primary" />
            <p className="text-sm">Crawling {websiteUrl}...</p>
          </div>
        )}

        {crawledUrls.length > 0 && (
          <>
            <p className="text-sm text-muted-foreground mb-4">
              Found <span className="font-semibold text-foreground">{crawledUrls.length}</span> pages from{" "}
              <span className="font-medium text-foreground break-all">{websiteUrl}</span>.
              Deselect pages you don't want embedded.
            </p>

            <div className="rounded-lg border border-border bg-muted/30 p-4 mb-4">
              <div className="flex items-center justify-between mb-3">
                <label className="flex items-center gap-2 cursor-pointer select-none">
                  <input
                    type="checkbox"
                    checked={totalChecked === crawledUrls.length && crawledUrls.length > 0}
                    onChange={toggleAll}
                    className="w-4 h-4 rounded accent-primary cursor-pointer"
                  />
                  <span className="text-sm font-medium text-foreground">
                    {totalChecked === crawledUrls.length ? "Deselect all" : "Select all"} ({totalChecked} of {crawledUrls.length})
                  </span>
                </label>
                <div className="flex gap-1">
                  <button
                    onClick={expandAllPaths}
                    className="px-2 py-1 text-xs rounded border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors flex items-center gap-1"
                  >
                    <ChevronsDownUp size={12} /> Expand All
                  </button>
                  <button
                    onClick={collapseAllPaths}
                    className="px-2 py-1 text-xs rounded border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors flex items-center gap-1"
                  >
                    <ChevronsUpDown size={12} /> Collapse All
                  </button>
                </div>
              </div>

              <div className="space-y-1 border-t border-border pt-2">
                {urlGroups.map((group) => {
                  const groupChecked = group.urls.filter(u => checked.has(u)).length;
                  const allGroupChecked = groupChecked === group.urls.length;
                  const isHostCollapsed = collapsedHosts.has(group.hostname);
                  const tree = pathTrees.get(group.hostname) ?? [];

                  return (
                    <div key={group.hostname}>
                      <div className="flex items-center gap-2 py-1.5 hover:bg-muted/30 rounded px-1">
                        <button
                          onClick={() => toggleHostCollapse(group.hostname)}
                          className="text-muted-foreground hover:text-foreground shrink-0"
                        >
                          {isHostCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                        </button>
                        <input
                          type="checkbox"
                          checked={allGroupChecked}
                          ref={el => { if (el) el.indeterminate = groupChecked > 0 && groupChecked < group.urls.length; }}
                          onChange={() => toggleUrlSet(group.urls)}
                          className="w-4 h-4 rounded accent-primary cursor-pointer shrink-0"
                        />
                        <span className="text-xs font-medium text-foreground truncate">
                          {group.hostname}
                        </span>
                        <span className="text-xs text-muted-foreground ml-auto shrink-0">
                          {groupChecked}/{group.urls.length}
                        </span>
                      </div>

                      {!isHostCollapsed && (
                        <div className="ml-6 space-y-0.5 border-l border-border/50 pl-3">
                          <PathTreeList
                            nodes={tree}
                            checked={checked}
                            collapsedPaths={collapsedPaths}
                            branchLeaves={branchLeaves}
                            onToggleUrl={toggleUrl}
                            onToggleUrlSet={toggleUrlSet}
                            onToggleCollapse={togglePathCollapse}
                          />
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="flex gap-3">
              <button onClick={onStartAnother} className={btnSecondary}>
                Cancel
              </button>
              <button
                disabled={totalChecked === 0}
                onClick={() => onEmbed(Array.from(checked))}
                className={btnPrimary}
              >
                Embed {totalChecked} page{totalChecked !== 1 ? "s" : ""}
              </button>
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="flex items-center gap-3 mb-4">
        <div className="p-2 rounded-lg bg-primary/15">
          <Globe size={22} className="text-primary" />
        </div>
        <h2 className="text-lg font-semibold text-foreground">Embedding Website</h2>
      </div>

      {isEmbedding && (
        <>
          <p className="text-sm text-muted-foreground mb-4">
            Embedding {totalChecked} page{totalChecked !== 1 && "s"} into group <span className="font-medium text-foreground">{groupName}</span>.
          </p>
          <div className="rounded-lg border border-border bg-muted/30 p-4 mb-4">
            <div className="w-full bg-muted rounded-full h-3 overflow-hidden">
              <div
                className="bg-primary h-full rounded-full transition-[width] duration-300"
                style={{ width: `${embedJob?.progress_percentage ?? 0}%` }}
              />
            </div>
            <p className="text-xs text-muted-foreground mt-2 text-center">
              {embedJob?.progress_percentage ?? 0}%
            </p>
          </div>
        </>
      )}

      {isEmbedDone && (
        <div className="rounded-lg border border-border bg-success/5 p-4 mb-6 space-y-2 text-sm">
          <div className="flex items-center gap-2 text-success">
            <CheckCircle size={18} />
            <span className="font-semibold">Embedding complete</span>
          </div>
          <p className="text-muted-foreground">
            {totalChecked} page{totalChecked !== 1 && "s"} embedded into group{" "}
            <span className="font-medium text-foreground">{groupName}</span>.
          </p>
        </div>
      )}

      {!isEmbedding && !isEmbedDone && (
        <p className="text-sm text-muted-foreground mb-4">Waiting for embedding to start...</p>
      )}

      <button onClick={onStartAnother} className={btnPrimary}>
        <RefreshCw size={16} /> Start Another
      </button>
    </div>
  );
};

export default WebsiteProcessingStep;
