import { useState, useCallback } from "react";
import type { ToolFormData } from "@/types/mcp";
import { emptyToolForm } from "@/types/mcp";

// Manages tool form state with scope toggle helpers
export function useToolForm(initial?: ToolFormData) {
  const [form, setForm] = useState<ToolFormData>(initial ?? emptyToolForm());

  const updateForm = useCallback((updates: Partial<ToolFormData>) => {
    setForm((prev) => ({ ...prev, ...updates }));
  }, []);

  const toggleRepo = useCallback((repo: string) => {
    setForm((prev) => {
      const next = new Set(prev.selectedRepos);
      if (next.has(repo)) next.delete(repo); else next.add(repo);
      return { ...prev, selectedRepos: next };
    });
  }, []);

  const toggleGroup = useCallback((group: string) => {
    setForm((prev) => {
      const next = new Set(prev.selectedGroups);
      if (next.has(group)) next.delete(group); else next.add(group);
      return { ...prev, selectedGroups: next };
    });
  }, []);

  const setRepos = useCallback((repos: Set<string>) => {
    setForm((prev) => ({ ...prev, selectedRepos: repos }));
  }, []);

  const setGroups = useCallback((groups: Set<string>) => {
    setForm((prev) => ({ ...prev, selectedGroups: groups }));
  }, []);

  const toggleWebsite = useCallback((website: string) => {
    setForm((prev) => {
      const next = new Set(prev.selectedWebsites);
      if (next.has(website)) next.delete(website); else next.add(website);
      return { ...prev, selectedWebsites: next };
    });
  }, []);

  const setWebsites = useCallback((websites: Set<string>) => {
    setForm((prev) => ({ ...prev, selectedWebsites: websites }));
  }, []);

  const resetForm = useCallback(() => {
    setForm(emptyToolForm());
  }, []);

  return { form, updateForm, toggleRepo, toggleGroup, setRepos, setGroups, toggleWebsite, setWebsites, resetForm };
}
