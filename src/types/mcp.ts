export interface McpServer {
  id: string;
  user_id: string;
  name: string;
  description: string | null;
  active: boolean;
  created_at: string;
  updated_at: string;
  tools?: ToolDefinition[];
}

export interface ToolDefinition {
  id: string;
  user_id: string;
  mcp_server_id: string;
  name: string;
  description: string | null;
  active: boolean;
  created_at: string;
  updated_at: string;
  code_search_scopes: ToolCodeSearchScope[];
  document_search_scopes: ToolDocumentSearchScope[];
}

export interface ToolCodeSearchScope {
  id: string;
  tool_definition_id: string;
  collection: string;
  repo_names: string[];
}

export interface ToolDocumentSearchScope {
  id: string;
  tool_definition_id: string;
  collection: string;
  group_ids: string[];
}

export interface ToolPayload {
  name: string;
  description: string;
  code_search_scopes: { collection: string; repo_names: string[] }[];
  document_search_scopes: { collection: string; group_ids: string[] }[];
}

export interface ToolFormData {
  name: string;
  description: string;
  selectedRepos: Set<string>;
  selectedGroups: Set<string>;
  selectedWebsites: Set<string>;
}

export const emptyToolForm = (): ToolFormData => ({
  name: "",
  description: "",
  selectedRepos: new Set(),
  selectedGroups: new Set(),
  selectedWebsites: new Set(),
});

export const toolFormToPayload = (form: ToolFormData): ToolPayload => {
  const groupIds = new Set(form.selectedGroups);
  form.selectedWebsites.forEach((w) => groupIds.add(w));
  return {
    name: form.name.trim(),
    description: form.description.trim(),
    code_search_scopes:
      form.selectedRepos.size > 0
        ? [{ collection: "codebase", repo_names: Array.from(form.selectedRepos) }]
        : [],
    document_search_scopes:
      groupIds.size > 0
        ? [{ collection: "general", group_ids: Array.from(groupIds) }]
        : [],
  };
};

export const toolToFormData = (tool: ToolDefinition, knownWebsiteGroups: string[] = []): ToolFormData => {
  const repos = new Set<string>();
  tool.code_search_scopes.forEach((s) => s.repo_names.forEach((r) => repos.add(r)));
  const groups = new Set<string>();
  const websites = new Set<string>();
  const websiteSet = new Set(knownWebsiteGroups);
  tool.document_search_scopes.forEach((s) => s.group_ids.forEach((g) => {
    if (websiteSet.has(g)) {
      websites.add(g);
    } else {
      groups.add(g);
    }
  }));
  return { name: tool.name, description: tool.description || "", selectedRepos: repos, selectedGroups: groups, selectedWebsites: websites };
};

export interface ConnectionInfo {
  mcp_url: string;
  user_id: string;
  server_id: string;
  server_name: string;
  full_url: string;
  config_snippets: {
    claude_desktop: object;
    opencode: object;
    vscode: object;
  };
}
