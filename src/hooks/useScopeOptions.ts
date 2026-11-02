import { useState, useEffect } from "react";
import { getMetadataValues, getWebsites } from "@/utils/apicalls";

// Fetches available repo names, document group IDs, and website hostnames for scope selection
export function useScopeOptions() {
  const [repoOptions, setRepoOptions] = useState<string[]>([]);
  const [groupOptions, setGroupOptions] = useState<string[]>([]);
  const [websiteOptions, setWebsiteOptions] = useState<string[]>([]);

  useEffect(() => {
    getMetadataValues("codebase", "repo_name")
      .then((res) => setRepoOptions([...res.values].sort()))
      .catch(() => setRepoOptions([]));

    getMetadataValues("general", "group")
      .then(async (res) => {
        const allGroups = [...res.values].sort();
        try {
          const { websites } = await getWebsites();
          const websiteHostnames = [...new Set(websites.map((w) => w.group).filter(Boolean))].sort();
          setWebsiteOptions(websiteHostnames);
          setGroupOptions(allGroups.filter((g) => !websiteHostnames.includes(g)));
        } catch {
          setWebsiteOptions([]);
          setGroupOptions(allGroups);
        }
      })
      .catch(() => {
        setGroupOptions([]);
        setWebsiteOptions([]);
      });
  }, []);

  return { repoOptions, groupOptions, websiteOptions };
}
