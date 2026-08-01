import React from "react";
import { EmbedJob } from "@/types/embed";
import { statusDot } from "@/styles/classes";

interface Props {
  embedJob: EmbedJob;
}

const JobStatusCompleteRow: React.FC<Props> = ({ embedJob }) => {
  const failed = embedJob.status === "FAILED";
  const displayName = embedJob.file_name?.trim() || "Untitled";

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-3">
        <span
          className={`${statusDot} ${failed ? "bg-destructive" : "bg-success"}`}
        />
        <span
          className="text-sm text-foreground truncate"
          title={displayName}
        >
          {displayName}
        </span>
        <span
          className={`text-xs uppercase ${failed ? "text-destructive" : "text-success"}`}
        >
          {failed ? "Failed" : "Completed"}
        </span>
      </div>
      {failed && embedJob.message && (
        <p
          className="text-xs text-destructive pl-5 truncate"
          title={embedJob.message}
        >
          {embedJob.message}
        </p>
      )}
    </div>
  );
};

export default JobStatusCompleteRow;
