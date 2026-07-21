import React from "react";
import { Code2, FileText, Globe } from "lucide-react";
import { wizardCard, wizardCardUnselected } from "@/styles/classes";

interface Props {
  onSelect: (type: "codebase" | "general" | "website") => void;
}

const EmbeddingTypeStep: React.FC<Props> = ({ onSelect }) => (
  <div>
    <h2 className="text-lg font-semibold text-foreground mb-1">What are you embedding?</h2>
    <p className="text-sm text-muted-foreground mb-5">
      Choose the type of content you're uploading. This determines how your files are processed and indexed.
    </p>

    <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
      <button
        type="button"
        onClick={() => onSelect("codebase")}
        className={`${wizardCard} ${wizardCardUnselected} text-left flex flex-col`}
      >
        <div className="flex items-center gap-3 mb-3">
          <div className="p-2 rounded-lg bg-primary/15">
            <Code2 size={22} className="text-primary" />
          </div>
          <span className="font-semibold text-foreground">Code / Code Repository</span>
        </div>
        <div className="flex-1 flex items-center">
          <p className="text-sm text-muted-foreground leading-relaxed">
            Code-aware embedding that understands syntax and file relationships for better codebase search.
          </p>
        </div>
      </button>

      <button
        type="button"
        onClick={() => onSelect("general")}
        className={`${wizardCard} ${wizardCardUnselected} text-left flex flex-col`}
      >
        <div className="flex items-center gap-3 mb-3">
          <div className="p-2 rounded-lg bg-primary/15">
            <FileText size={22} className="text-primary" />
          </div>
          <span className="font-semibold text-foreground">General Documents</span>
        </div>
        <div className="flex-1 flex items-center">
          <p className="text-sm text-muted-foreground leading-relaxed">
            Text embedding for docs, articles, CSVs, and other non-code files.
          </p>
        </div>
      </button>

      <button
        type="button"
        onClick={() => onSelect("website")}
        className={`${wizardCard} ${wizardCardUnselected} text-left flex flex-col`}
      >
        <div className="flex items-center gap-3 mb-3">
          <div className="p-2 rounded-lg bg-primary/15">
            <Globe size={22} className="text-primary" />
          </div>
          <span className="font-semibold text-foreground">Scrape Website</span>
        </div>
        <div className="flex-1 flex items-center">
          <p className="text-sm text-muted-foreground leading-relaxed">
            Crawl and embed content from any URL — ideal for documentation sites, API references, wiki pages, or Stack Overflow answers you want to search later.
          </p>
        </div>
      </button>
    </div>
  </div>
);

export default EmbeddingTypeStep;
