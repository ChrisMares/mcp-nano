import React from "react";
import { EmbedJob } from "@/types/embed";
import { statusDot } from "@/styles/classes";

interface Props {
  embedJob: EmbedJob;
}

const JobStatusCompleteRow: React.FC<Props> = ({ embedJob }) => {
  const failed = embedJob.status === "FAILED";
  const cancelled = embedJob.status === "CANCELLED";
  const displayName = embedJob.file_name?.trim() || "Untitled";

  const dotClass = failed
    ? "bg-destructive"
    : cancelled
      ? "bg-warning"
      : "bg-success";
  const label = failed ? "Failed" : cancelled ? "Cancelled" : "Completed";
  const labelClass = failed
    ? "text-destructive"
    : cancelled
      ? "text-muted-foreground"
      : "text-success";

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-3">
        <span className={`${statusDot} ${dotClass}`} />
        <span
          className="text-sm text-foreground truncate"
          title={displayName}
        >
          {displayName}
        </span>
        <span className={`text-xs uppercase ${labelClass}`}>{label}</span>
      </div>
      {(failed || cancelled) && embedJob.message && (
        <p
          className={`text-xs pl-5 truncate ${failed ? "text-destructive" : "text-muted-foreground"}`}
          title={embedJob.message}
        >
          {embedJob.message}
        </p>
      )}
    </div>
  );
};

export default JobStatusCompleteRow;
