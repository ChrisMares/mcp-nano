import React, { useEffect, useState } from "react";
import { EmbedJob } from "@/types/embed";
import { RotateCw } from "lucide-react";
import { statusDot } from "@/styles/classes";

interface Props {
  embedJob: EmbedJob;
}

const JobStatusRow: React.FC<Props> = ({ embedJob }) => {
  const [percentComplete, setPercentComplete] = useState<number>(
    embedJob.progress_percentage || 0,
  );

  useEffect(() => {
    const incoming = embedJob.progress_percentage ?? 0;

    // Progress bars only move forward; stale events can't rewind them.
    setPercentComplete((prev) => (incoming > prev ? incoming : prev));
  }, [embedJob.progress_percentage]);

  const displayName = embedJob.file_name?.trim() || "Untitled";

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-3">
        <span
          className={`${statusDot} ${embedJob.status === "RUNNING" ? "bg-info" : "bg-warning"}`}
        />
        <span
          className="text-sm text-foreground truncate"
          title={displayName}
        >
          {displayName}
        </span>
        <span className="text-xs text-muted-foreground uppercase">
          {embedJob.status}
        </span>
        {embedJob.status === "PENDING" && embedJob.queue_position != null && (
          <span className="text-xs text-muted-foreground">
            #{embedJob.queue_position} of {embedJob.total_in_queue} in queue
          </span>
        )}
        {embedJob.status === "PENDING" && embedJob.queue_position == null && (
          <RotateCw size={14} className="text-muted-foreground" />
        )}
        {embedJob.status === "RUNNING" && (
          <div className="flex-1 flex items-center gap-2 min-w-0">
            <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
              <div
                className="h-full bg-info rounded-full transition-[width] duration-300"
                style={{ width: `${percentComplete}%` }}
              />
            </div>
            <span className="text-xs text-muted-foreground w-8 text-right">
              {percentComplete}%
            </span>
          </div>
        )}
      </div>
      {embedJob.message && (
        <p
          className="text-xs text-muted-foreground pl-5 truncate"
          title={embedJob.message}
        >
          {embedJob.message}
        </p>
      )}
    </div>
  );
};

export default JobStatusRow;
