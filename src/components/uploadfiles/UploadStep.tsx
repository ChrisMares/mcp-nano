import React, { useEffect } from "react";
import { ArrowLeft, CheckCircle, X } from "lucide-react";
import {
  Dropzone,
  DropzoneContent,
  DropzoneEmptyState,
} from "@/components/ui/dropzone";
import { type UseLocalUploadReturn } from "@/hooks/use-local-upload";
import { wizardNav, btnSecondary } from "@/styles/classes";

interface Props {
  collection: "codebase" | "general" | "";
  codeUploadMode: "zip" | "individual" | "";
  repoName: string;
  groupName: string;
  uploadProps: UseLocalUploadReturn;
  disableUpload: boolean;
  disableReason: string;
  mixWarning: string;
  lastSubmittedCount: number;
  onDismissSubmitted: () => void;
  onBack: () => void;
}

const UploadStep: React.FC<Props> = ({
  collection, codeUploadMode, repoName, groupName, uploadProps,
  disableUpload, disableReason, mixWarning, lastSubmittedCount, onDismissSubmitted, onBack,
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
        <p>Collection: <span className="font-medium text-foreground">{collection}</span></p>
        {collection === "codebase" && codeUploadMode === "zip" && (
          <p>Upload zip files <span className="text-xs">(zip filename becomes the repo name)</span></p>
        )}
        {collection === "codebase" && codeUploadMode === "individual" && repoName && (
          <p>Uploading to repo: <span className="font-medium text-foreground">"{repoName}"</span></p>
        )}
        {collection === "general" && groupName && (
          <p>Document group: <span className="font-medium text-foreground">"{groupName}"</span></p>
        )}
      </div>

      {lastSubmittedCount > 0 && (
        <div className="flex items-center gap-2 mb-4 px-3 py-2 rounded-lg bg-success/15 border border-success/30 text-sm text-success">
          <CheckCircle size={16} className="shrink-0" />
          <span>
            {lastSubmittedCount} file{lastSubmittedCount > 1 ? "s" : ""} submitted for processing. Track progress below.
          </span>
          <button onClick={onDismissSubmitted} className="ml-auto text-success/70 hover:text-success">
            <X size={14} />
          </button>
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
