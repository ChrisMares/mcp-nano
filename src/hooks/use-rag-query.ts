/**
 * State and actions for the Fetch Context page: collection and scope
 * filters, and either a semantic search or a code symbol lookup.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getMetadataValues, ragQuery } from "@/utils/apicalls";
import { errorMessage } from "@/lib/utils";
import type { RagQueryPayload, RagResponse, SymbolRelation } from "@/types/rag";

export type RagCollection = "codebase" | "general";
export type SearchMode = "semantic" | "symbol";

/** Fetch Context page state. Symbol lookups only apply to the codebase collection. */
export function useRagQuery() {
  const [collection, setCollectionState] = useState<RagCollection>("codebase");
  const [mode, setMode] = useState<SearchMode>("semantic");
  const [query, setQuery] = useState("");
  const [symbol, setSymbol] = useState("");
  const [relation, setRelation] = useState<SymbolRelation>("definition");
  const [expand, setExpand] = useState(false);
  const [limit, setLimit] = useState(10);
  const [filterOptions, setFilterOptions] = useState<string[]>([]);
  const [selectedFilters, setSelectedFilters] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [response, setResponse] = useState<RagResponse | undefined>(undefined);
  const requestId = useRef(0);

  useEffect(() => {
    let cancelled = false;
    const key = collection === "codebase" ? "repo_name" : "group";
    getMetadataValues(collection, key)
      .then((res) => [...res.values].sort())
      .catch(() => [])
      .then((options) => {
        if (!cancelled) setFilterOptions(options);
      });
    return () => {
      cancelled = true;
    };
  }, [collection]);

  const setCollection = useCallback((next: RagCollection) => {
    requestId.current += 1;
    setCollectionState(next);
    setLoading(false);
    setSelectedFilters(new Set());
    setResponse(undefined);
    setError(null);
    if (next !== "codebase") setMode("semantic");
  }, []);

  const toggleFilter = useCallback((name: string) => {
    setSelectedFilters((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const symbolMode = collection === "codebase" && mode === "symbol";
  const canSubmit = !loading && (symbolMode ? symbol.trim() !== "" : query.trim() !== "");

  const submit = useCallback(async () => {
    if (!canSubmit) return;
    const payload: RagQueryPayload = {
      collection,
      query: query.trim(),
      show_documents: false,
      limit,
    };
    if (symbolMode) {
      payload.symbol = symbol.trim();
      payload.relation = relation;
    } else if (collection === "codebase" && expand) {
      payload.expand = true;
    }
    if (selectedFilters.size > 0) {
      const filterKey = collection === "codebase" ? "repo_name" : "group";
      payload.where = { $or: [...selectedFilters].map((value) => ({ [filterKey]: value })) };
    }

    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    setResponse(undefined);
    try {
      const result = await ragQuery(payload);
      if (id === requestId.current) setResponse(result);
    } catch (err: unknown) {
      if (id === requestId.current) setError(errorMessage(err));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, [canSubmit, collection, expand, limit, query, relation, selectedFilters, symbol, symbolMode]);

  return {
    collection, setCollection, mode, setMode, symbolMode,
    query, setQuery, symbol, setSymbol, relation, setRelation,
    expand, setExpand, limit, setLimit,
    filterOptions, selectedFilters, toggleFilter,
    loading, error, canSubmit, submit, response,
  };
}
