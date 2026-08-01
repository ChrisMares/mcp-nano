import { useState, useCallback } from "react";
import type { ToolFormData } from "@/types/mcp";
import { emptyToolForm } from "@/types/mcp";

type SelectionKey = "selectedRepos" | "selectedGroups" | "selectedWebsites";

// Manages tool form state with scope toggle helpers
export function useToolForm(initial?: ToolFormData) {
  const [form, setForm] = useState<ToolFormData>(initial ?? emptyToolForm());

  const updateForm = useCallback((updates: Partial<ToolFormData>) => {
    setForm((prev) => ({ ...prev, ...updates }));
  }, []);

  const toggleItem = useCallback((key: SelectionKey, item: string) => {
    setForm((prev) => {
      const next = new Set(prev[key]);
      if (next.has(item)) next.delete(item); else next.add(item);
      return { ...prev, [key]: next };
    });
  }, []);

  const setItems = useCallback((key: SelectionKey, items: Set<string>) => {
    setForm((prev) => ({ ...prev, [key]: items }));
  }, []);

  const toggleRepo = useCallback((repo: string) => toggleItem("selectedRepos", repo), [toggleItem]);
  const toggleGroup = useCallback((group: string) => toggleItem("selectedGroups", group), [toggleItem]);
  const toggleWebsite = useCallback((website: string) => toggleItem("selectedWebsites", website), [toggleItem]);

  const setRepos = useCallback((repos: Set<string>) => setItems("selectedRepos", repos), [setItems]);
  const setGroups = useCallback((groups: Set<string>) => setItems("selectedGroups", groups), [setItems]);
  const setWebsites = useCallback((websites: Set<string>) => setItems("selectedWebsites", websites), [setItems]);

  const resetForm = useCallback(() => {
    setForm(emptyToolForm());
  }, []);

  return { form, updateForm, toggleRepo, toggleGroup, setRepos, setGroups, toggleWebsite, setWebsites, resetForm };
}
