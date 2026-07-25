import React, { useState, useEffect, useCallback } from "react";
import { getMetadataValues, ragQuery } from "@/utils/apicalls";
import { Loader2 } from "lucide-react";
import {
  radioInput,
  textareaInput,
  btnPrimary,
  fieldLabel,
  sliderBounds,
} from "@/styles/classes";
import PageHead from "@/components/shared/PageHead";
import { useTheme } from "@/contexts/ThemeContext";
import type { RagQueryPayload, RagResponse } from "@/types/rag";
import RagResultsPanel from "@/components/fetchcontext/RagResultsPanel";
import FilterSelect from "@/components/fetchcontext/FilterSelect";

const SLIDER_CONFIG = [
  { key: "limit", label: "Results Limit", min: 5, max: 100, step: 5 },
] as const;

const FetchContext: React.FC = () => {
  console.log("Rendering FetchContext component");

  const { theme } = useTheme();
  const [collection, setCollection] = useState<"codebase" | "general">(
    "codebase",
  );
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(10);
  const [filterOptions, setFilterOptions] = useState<string[]>([]);
  const [selectedFilters, setSelectedFilters] = useState<Set<string>>(
    new Set(),
  );
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{
    rag_response?: RagResponse;
    error?: string;
  } | null>(null);

  const sliderValues: Record<string, number> = { limit };
  const sliderSetters: Record<string, (v: number) => void> = {
    limit: setLimit,
  };

  useEffect(() => {
    setSelectedFilters(new Set());
    const key = collection === "codebase" ? "repo_name" : "group";
    getMetadataValues(collection, key)
      .then((res) => {
        if (res?.values) {
          const sorted = [...res.values].sort();
          setFilterOptions(sorted);
        }
      })
      .catch(() => setFilterOptions([]));
  }, [collection]);

  const handleCollectionChange = (val: "codebase" | "general") => {
    setCollection(val);
    setResult(null);
  };

  const toggleFilter = (name: string) => {
    setSelectedFilters((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const handleSubmit = useCallback(async () => {
    if (!query.trim()) return;

    setLoading(true);
    setResult(null);

    const filterKey = collection === "codebase" ? "repo_name" : "group";
    const payload: RagQueryPayload = {
      collection,
      query: query.trim(),
      show_documents: false,
      limit,
    };

    if (selectedFilters.size > 0) {
      payload.where = {
        $or: Array.from(selectedFilters).map((v) => ({ [filterKey]: v })),
      };
    }

    try {
      const response = await ragQuery(payload);
      setResult({ rag_response: response });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setResult({ error: msg });
    } finally {
      setLoading(false);
    }
  }, [collection, query, limit, selectedFilters]);

  const filterLabel = collection === "codebase" ? "Repos" : "Groups";
  const canSubmit = query.trim().length > 0 && !loading;

  return (
    <div className="rounded-lg border border-border bg-card p-6">
      <PageHead
        title="Fetch Context – Semantic Search"
        description="Search embedded documents and code and return matching context chunks."
        path="/query/fetch"
      />
      <h1 className="text-2xl font-bold text-foreground mb-4">Fetch Context</h1>
      <p className="text-muted-foreground mb-6">
        Query your embedded data to retrieve relevant context.
      </p>

      <div className="max-w-3xl space-y-6">
        {/* Collection Radio */}
        <div>
          <label className="block text-sm font-medium text-foreground mb-3">
            Collection
          </label>
          <div className="flex gap-6">
            {(["codebase", "general"] as const).map((val) => (
              <label
                key={val}
                className="flex items-center gap-2 cursor-pointer"
              >
                <input
                  type="radio"
                  name="collection"
                  value={val}
                  checked={collection === val}
                  onChange={() => handleCollectionChange(val)}
                  className={radioInput}
                />
                <span className="text-sm text-foreground capitalize">
                  {val}
                </span>
              </label>
            ))}
          </div>
        </div>

        {filterOptions.length > 0 && (
          <FilterSelect
            label={filterLabel}
            options={filterOptions}
            selected={selectedFilters}
            onToggle={toggleFilter}
          />
        )}

        {/* Query */}
        <div>
          <label className={fieldLabel}>Query</label>
          <textarea
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className={textareaInput}
            placeholder="Enter your query..."
          />
        </div>

        {/* Sliders */}
        <div className="grid grid-cols-2 gap-6">
          {SLIDER_CONFIG.map(({ key, label, min, max, step }) => (
            <div key={key}>
              <label className={fieldLabel}>
                {label}:{" "}
                <span className="text-primary">{sliderValues[key]}</span>
              </label>
              <input
                type="range"
                min={min}
                max={max}
                step={step}
                value={sliderValues[key]}
                onChange={(e) => sliderSetters[key](Number(e.target.value))}
                className="w-full accent-primary"
              />
              <div className={sliderBounds}>
                <span>{min}</span>
                <span>{max}</span>
              </div>
            </div>
          ))}
        </div>

        <button
          onClick={handleSubmit}
          disabled={!canSubmit}
          className={btnPrimary}
        >
          {loading ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Fetching...
            </>
          ) : (
            "Fetch Context"
          )}
        </button>

        <RagResultsPanel data={result?.rag_response} theme={theme} />
      </div>
    </div>
  );
};

export default FetchContext;
