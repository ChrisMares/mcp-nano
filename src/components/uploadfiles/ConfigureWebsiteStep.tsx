import React from "react";
import { ArrowLeft, ArrowRight, Loader2, X } from "lucide-react";
import {
  wizardNav,
  btnPrimary,
  btnSecondary,
  fieldLabel,
  textInput,
  sliderBounds,
} from "@/styles/classes";

interface Props {
  websiteUrl: string;
  depth: number;
  sameDomainOnly: boolean;
  renderJavascript: boolean;
  isCrawling: boolean;
  crawlCurrentUrl?: string | null;
  crawlFoundCount?: number;
  onUrlChange: (url: string) => void;
  onDepthChange: (depth: number) => void;
  onSameDomainChange: (val: boolean) => void;
  onRenderJavascriptChange: (val: boolean) => void;
  onCancelCrawl: () => void;
  onBack: () => void;
  onNext: () => void;
}

const DEPTH_LABELS = [
  "This page only",
  "1 level deep",
  "2 levels deep",
  "3 levels deep",
  "4 levels deep",
  "5 levels deep",
];

const ConfigureWebsiteStep: React.FC<Props> = ({
  websiteUrl,
  depth,
  sameDomainOnly,
  renderJavascript,
  isCrawling,
  crawlCurrentUrl = null,
  crawlFoundCount = 0,
  onUrlChange,
  onDepthChange,
  onSameDomainChange,
  onRenderJavascriptChange,
  onCancelCrawl,
  onBack,
  onNext,
}) => {
  const canProceed = !!websiteUrl.trim() && !isCrawling;

  return (
    <div>
      <h2 className="text-lg font-semibold text-foreground mb-1">
        Configure Website Crawl
      </h2>
      <p className="text-sm text-muted-foreground mb-5">
        Provide the URL to crawl. Sitemap URLs are included alongside links
        discovered from each page.
      </p>

      <div className="mb-5">
        <label className={fieldLabel}>
          Website URL <span className="text-destructive">*</span>
        </label>
        <input
          type="url"
          className={textInput}
          placeholder="https://docs.example.com"
          value={websiteUrl}
          onChange={(e) => onUrlChange(e.target.value)}
          disabled={isCrawling}
        />
        <p className="text-xs text-muted-foreground mt-1">
          The starting URL for sitemap and page-link discovery.
        </p>
      </div>

      <div className="mb-5">
        <label
          className={`${fieldLabel} flex items-center gap-2 cursor-pointer`}
          title="Use an installed Chromium-family browser to execute JavaScript before extracting links.
          Use this for websites that are SPA's and do not have server-rendered HTML. This is slower, but more thorough."
        >
          <input
            type="checkbox"
            checked={renderJavascript}
            onChange={(e) => onRenderJavascriptChange(e.target.checked)}
            className="w-4 h-4 rounded border-border text-primary focus:ring-brand-cyan"
            disabled={isCrawling}
          />
          Render JavaScript
        </label>
        <p className="text-xs text-muted-foreground mt-1 ml-6">
          Client-rendered navigation if any Chromium browser is installed.
          Slower crawl, but more thorough.
        </p>
      </div>

      <div className="mb-5">
        <label
          className={`${fieldLabel} flex items-center gap-2 cursor-pointer`}
          title="When checked, the crawler stays on the starting host and path subtree. Uncheck to allow crawling outside that scope."
        >
          <input
            type="checkbox"
            checked={sameDomainOnly}
            onChange={(e) => onSameDomainChange(e.target.checked)}
            className="w-4 h-4 rounded border-border text-primary focus:ring-brand-cyan"
            disabled={isCrawling}
          />
          Only crawl current site section
        </label>
        <p className="text-xs text-muted-foreground mt-1 ml-6">
          When checked, only URLs under the starting host and path are crawled.
          For example, /docs/ excludes /blog/.
        </p>
      </div>

      <div className="mb-6">
        <label
          className={fieldLabel}
          title="The number of link levels to follow from the starting URL. Higher depth means the crawler explores more pages, but takes longer."
        >
          Crawl Depth —{" "}
          <span className="text-foreground font-semibold">
            {DEPTH_LABELS[depth]}
          </span>
        </label>
        <input
          type="range"
          min={0}
          max={5}
          step={1}
          value={depth}
          onChange={(e) => onDepthChange(Number(e.target.value))}
          className="w-full h-2 rounded-full appearance-none cursor-pointer accent-primary bg-muted"
          disabled={isCrawling}
        />
        <div className={sliderBounds}>
          <span>0 — page only</span>
          <span>5 — deep crawl</span>
        </div>
        <p className="text-xs text-muted-foreground mt-1">
          Sitemap URLs are not limited by this depth. Higher depth adds more
          page-link discovery.
        </p>
      </div>

      {isCrawling && (
        <div className="flex items-center gap-2 mb-4 text-sm text-muted-foreground min-w-0">
          <button
            type="button"
            onClick={onCancelCrawl}
            title="cancel"
            aria-label="cancel crawl"
            className="shrink-0 flex items-center justify-center w-6 h-6 rounded-md border border-border text-foreground font-bold hover:bg-muted hover:border-foreground"
          >
            <X size={16} strokeWidth={3} />
          </button>
          <Loader2 size={16} className="animate-spin text-primary shrink-0" />
          <span className="truncate" title={crawlCurrentUrl ?? undefined}>
            {crawlCurrentUrl
              ? `Crawling ${crawlCurrentUrl}`
              : "Crawl in progress…"}
            {crawlFoundCount > 0 ? ` · ${crawlFoundCount} found` : ""}
          </span>
        </div>
      )}

      <div className={wizardNav}>
        <button onClick={onBack} className={btnSecondary}>
          <ArrowLeft size={16} /> Back
        </button>
        <button disabled={!canProceed} onClick={onNext} className={btnPrimary}>
          {isCrawling ? (
            <>
              <Loader2 size={16} className="animate-spin" /> Crawling...
            </>
          ) : (
            <>
              Start Crawl <ArrowRight size={16} />
            </>
          )}
        </button>
      </div>
    </div>
  );
};

export default ConfigureWebsiteStep;
