import React, { useEffect } from "react";
import { ArrowLeft, CheckCircle, X } from "lucide-react";
import { Dropzone, DropzoneContent, DropzoneEmptyState } from "@/components/ui/dropzone";
import { type UseLocalUploadReturn } from "@/hooks/use-local-upload";
import { wizardNav, btnSecondary } from "@/styles/classes";

interface Props {
  collection: "codebase" | "general" | "";
  codeUploadMode: "zip" | "individual" | "";
  repoName: string;
  groupName: string;
  filterTestFiles: boolean;
  onFilterTestFilesChange: (val: boolean) => void;
  codeFilesOnly: boolean;
  onCodeFilesOnlyChange: (val: boolean) => void;
  uploadProps: UseLocalUploadReturn;
  disableUpload: boolean;
  disableReason: string;
  mixWarning: string;
  lastSubmittedCount: number;
  onDismissSubmitted: () => void;
  onBack: () => void;
}

const UploadStep: React.FC<Props> = ({
  collection,
  codeUploadMode,
  repoName,
  groupName,
  filterTestFiles,
  onFilterTestFilesChange,
  codeFilesOnly,
  onCodeFilesOnlyChange,
  uploadProps,
  disableUpload,
  disableReason,
  mixWarning,
  lastSubmittedCount,
  onDismissSubmitted,
  onBack,
}) => {
  // Auto-dismiss the submitted banner after 6 seconds
  useEffect(() => {
    if (lastSubmittedCount <= 0) return;
    const t = setTimeout(onDismissSubmitted, 6000);
    return () => clearTimeout(t);
  }, [lastSubmittedCount, onDismissSubmitted]);

  return (
    <div>
      <h2 className="text-lg font-semibold text-foreground mb-1">Upload Your Files</h2>
      <div className="text-sm text-muted-foreground mb-5 space-y-0.5">
        <p>
          Collection: <span className="font-medium text-foreground">{collection}</span>
        </p>
        {collection === "codebase" && codeUploadMode === "zip" && (
          <p>
            Upload zip files <span className="text-xs">(zip filename becomes the repo name)</span>
          </p>
        )}
        {collection === "codebase" && codeUploadMode === "individual" && repoName && (
          <p>
            Uploading to repo: <span className="font-medium text-foreground">"{repoName}"</span>
          </p>
        )}
        {collection === "general" && groupName && (
          <p>
            Document group: <span className="font-medium text-foreground">"{groupName}"</span>
          </p>
        )}
      </div>

      {collection === "codebase" && codeUploadMode === "zip" && (
        <div className="mb-1 space-y-4 flex">
          <div>
            <label
              className="flex items-center gap-2 cursor-pointer text-sm text-foreground"
              title="When checked, only source code, documentation and config files are embedded. Data and asset files (.csv, .jsonl, .xlsx, .pdf, images, archives, databases, keys, …) inside the zip are skipped. Uncheck to embed every file type."
            >
              <input
                type="checkbox"
                checked={codeFilesOnly}
                onChange={(e) => onCodeFilesOnlyChange(e.target.checked)}
                className="w-4 h-4 rounded border-border text-primary focus:ring-brand-cyan"
              />
              Code files only
            </label>
          </div>

          <div className="pl-5">
            <label
              className="flex items-center gap-2 cursor-pointer text-sm text-foreground"
              title="When checked, test files (foo.test.ts, test_bar.py, *_test.go, etc.) and test directories (test/, tests/, __tests__/, spec/, …) inside the zip are not embedded. Uncheck to include test code alongside the rest."
            >
              <input
                type="checkbox"
                checked={filterTestFiles}
                onChange={(e) => onFilterTestFilesChange(e.target.checked)}
                className="w-4 h-4 rounded border-border text-primary focus:ring-brand-cyan"
              />
              Filter out test files
            </label>
          </div>
        </div>
      )}

      <Dropzone {...uploadProps} disableUpload={disableUpload} disableReason={disableReason}>
        <DropzoneEmptyState />
        <DropzoneContent />
      </Dropzone>

      {mixWarning && <p className="text-sm text-destructive mt-2">{mixWarning}</p>}

      <div className={wizardNav}>
        <button onClick={onBack} className={btnSecondary}>
          <ArrowLeft size={16} /> Back
        </button>
        <div />
      </div>
    </div>
  );
};

export default UploadStep;
