import { useState, useEffect } from "react";
import { getMetadataValues, getWebsites } from "@/utils/apicalls";

interface WebsiteItem {
  group: string;
}

// Fetches available repo names, document group IDs, and website hostnames for scope selection
export function useScopeOptions() {
  const [repoOptions, setRepoOptions] = useState<string[]>([]);
  const [groupOptions, setGroupOptions] = useState<string[]>([]);
  const [websiteOptions, setWebsiteOptions] = useState<string[]>([]);

  useEffect(() => {
    getMetadataValues("codebase", "repo_name")
      .then((res) => { if (res?.values) { const sorted = [...res.values].sort(); setRepoOptions(sorted); } })
      .catch(() => setRepoOptions([]));

    getMetadataValues("general", "group")
      .then((res) => {
        if (res?.values) res.values.sort();
        return res?.values || [];
      })
      .then((allGroups: string[]) => {
        return getWebsites().then((webRes) => {
          const websites: WebsiteItem[] = webRes?.websites || [];
          const websiteHostnames = [...new Set(websites.map((w) => w.group).filter(Boolean))];
          setWebsiteOptions(websiteHostnames.sort());
          const websiteSet = new Set(websiteHostnames);
          setGroupOptions(allGroups.filter((g) => !websiteSet.has(g)));
        }).catch(() => {
          setWebsiteOptions([]);
          setGroupOptions(allGroups);
        });
      })
      .catch(() => {
        setGroupOptions([]);
        setWebsiteOptions([]);
      });
  }, []);

  return { repoOptions, groupOptions, websiteOptions };
}
