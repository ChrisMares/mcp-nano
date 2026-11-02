/** Fetch Context page: semantic search and code symbol lookup over embedded data. */
import React from "react";
import { Loader2 } from "lucide-react";
import {
  radioInput,
  textareaInput,
  textInput,
  selectInput,
  btnPrimary,
  fieldLabel,
  sliderBounds,
  alertError,
} from "@/styles/classes";
import PageHead from "@/components/shared/PageHead";
import { useTheme } from "@/contexts/ThemeContext";
import { useRagQuery, type RagCollection, type SearchMode } from "@/hooks/use-rag-query";
import type { SymbolRelation } from "@/types/rag";
import RagResultsPanel from "@/components/fetchcontext/RagResultsPanel";
import FilterSelect from "@/components/fetchcontext/FilterSelect";

const COLLECTIONS: RagCollection[] = ["codebase", "general"];

const MODES: { value: SearchMode; label: string }[] = [
  { value: "semantic", label: "Semantic search" },
  { value: "symbol", label: "Symbol lookup" },
];

const RELATIONS: { value: SymbolRelation; label: string }[] = [
  { value: "definition", label: "Definition" },
  { value: "references", label: "References (callers)" },
  { value: "callees", label: "Callees" },
  { value: "members", label: "Members (class/interface)" },
];

const isRelation = (value: string): value is SymbolRelation =>
  RELATIONS.some((r) => r.value === value);

/** Search form plus results; state lives in {@link useRagQuery}. */
const FetchContext: React.FC = () => {
  const { theme } = useTheme();
  const rag = useRagQuery();
  const isCode = rag.collection === "codebase";
  const { symbolMode } = rag;

  return (
    <div className="rounded-lg border border-border bg-card p-6">
      <PageHead
        title="Fetch Context – Semantic Search"
        description="Search embedded documents and code and return matching context chunks."
      />
      <h1 className="text-2xl font-bold text-foreground mb-4">Fetch Context</h1>
      <p className="text-muted-foreground mb-6">
        Query your embedded data to retrieve relevant context.
      </p>

      <div className="max-w-3xl space-y-6">
        <div>
          <label className="block text-sm font-medium text-foreground mb-3">Collection</label>
          <div className="flex gap-6">
            {COLLECTIONS.map((val) => (
              <label key={val} className="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="collection"
                  value={val}
                  checked={rag.collection === val}
                  onChange={() => rag.setCollection(val)}
                  className={radioInput}
                />
                <span className="text-sm text-foreground capitalize">{val}</span>
              </label>
            ))}
          </div>
        </div>

        {isCode && (
          <div role="radiogroup" aria-label="Search mode" className="inline-flex rounded-md border border-border p-0.5">
            {MODES.map((m) => (
              <button
                key={m.value}
                type="button"
                role="radio"
                aria-checked={rag.mode === m.value}
                onClick={() => rag.setMode(m.value)}
                className={`rounded px-3 py-1.5 text-sm ${
                  rag.mode === m.value
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {m.label}
              </button>
            ))}
          </div>
        )}

        {rag.filterOptions.length > 0 && (
          <FilterSelect
            label={isCode ? "Repos" : "Groups"}
            options={rag.filterOptions}
            selected={rag.selectedFilters}
            onToggle={rag.toggleFilter}
          />
        )}

        {symbolMode && (
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label htmlFor="symbol-input" className={fieldLabel}>Symbol</label>
              <input
                id="symbol-input"
                value={rag.symbol}
                onChange={(e) => rag.setSymbol(e.target.value)}
                className={textInput}
                placeholder="run_rag_query or RagService::run_rag_query"
              />
            </div>
            <div>
              <label htmlFor="relation-select" className={fieldLabel}>Relation</label>
              <select
                id="relation-select"
                value={rag.relation}
                onChange={(e) => {
                  if (isRelation(e.target.value)) rag.setRelation(e.target.value);
                }}
                className={selectInput}
              >
                {RELATIONS.map((r) => (
                  <option key={r.value} value={r.value}>{r.label}</option>
                ))}
              </select>
            </div>
          </div>
        )}

        <div>
          <label htmlFor="query-input" className={fieldLabel}>
            {symbolMode ? "Rank by (optional)" : "Query"}
          </label>
          <textarea
            id="query-input"
            value={rag.query}
            onChange={(e) => rag.setQuery(e.target.value)}
            className={textareaInput}
            placeholder={symbolMode ? "Optional text to rank matches..." : "Enter your query..."}
          />
        </div>

        <div className="grid grid-cols-2 gap-6">
          <div>
            <label className={fieldLabel}>
              Results Limit: <span className="text-primary">{rag.limit}</span>
            </label>
            <input
              type="range"
              min={5}
              max={100}
              step={5}
              value={rag.limit}
              onChange={(e) => rag.setLimit(Number(e.target.value))}
              className="w-full accent-primary"
            />
            <div className={sliderBounds}>
              <span>5</span>
              <span>100</span>
            </div>
          </div>
          {isCode && !symbolMode && (
            <label className="flex items-center gap-2 self-center cursor-pointer text-sm text-foreground">
              <input
                type="checkbox"
                checked={rag.expand}
                onChange={(e) => rag.setExpand(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-border text-primary"
              />
              Expand callees
            </label>
          )}
        </div>

        <button onClick={rag.submit} disabled={!rag.canSubmit} className={btnPrimary}>
          {rag.loading ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Fetching...
            </>
          ) : symbolMode ? (
            "Look up"
          ) : (
            "Fetch Context"
          )}
        </button>

        {rag.error && (
          <div className={alertError} role="alert">
            {rag.error}
          </div>
        )}

        <RagResultsPanel data={rag.response} theme={theme} />
      </div>
    </div>
  );
};

export default FetchContext;
