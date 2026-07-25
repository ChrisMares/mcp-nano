import React from "react";
import { Link } from "react-router-dom";
import { Loader2, RotateCw } from "lucide-react";
import { statusDot } from "@/styles/classes";
import type { EmbedJob } from "@/types/embed";

interface Props {
  processing: boolean;
  activeJobs: EmbedJob[];
  completedJobs: EmbedJob[];
}

const displayName = (job: EmbedJob) => job.file_name?.trim() || "Untitled";

const JobStatusPanel: React.FC<Props> = React.memo(({ processing, activeJobs, completedJobs }) => {
  const hasVisibleJobs = processing || activeJobs.length > 0 || completedJobs.length > 0;
  if (!hasVisibleJobs) return null;

  return (
    <div className="mt-8 p-4 rounded-lg border border-border bg-card">
      <h3 className="text-sm font-medium text-foreground mb-3">Embedding Status</h3>
      <p className="text-muted-foreground mb-6 text-sm">
        You may navigate away from this page, or upload more files. Your embedding tasks are queued up. Depending on server load and file sizes, processing time may vary.{" "}
        Once files are embedded you may view them in{" "}
        <Link to="/embed/data" className="text-cyan-700 hover:text-cyan-800 dark:text-cyan-400 dark:hover:text-cyan-300 underline underline-offset-2">
          Data Management
        </Link>
        .
      </p>
      <div className="space-y-3">
        {processing && activeJobs.length === 0 && completedJobs.length === 0 && (
          <div className="flex items-center gap-3">
            <Loader2 size={14} className="animate-spin text-info" />
            <span className="text-sm text-muted-foreground">Submitting files for processing...</span>
          </div>
        )}
        {activeJobs.map((job) => (
          <div key={job.job_id} className="space-y-1">
            <div className="flex items-center gap-3">
              <span className={`${statusDot} ${job.status === "RUNNING" ? "bg-info" : "bg-warning"}`} />
              <span className="text-sm text-foreground truncate" title={displayName(job)}>
                {displayName(job)}
              </span>
              <span className="text-xs text-muted-foreground uppercase">{job.status}</span>
              {job.status === "PENDING" && job.queue_position != null && (
                <span className="text-xs text-muted-foreground">
                  #{job.queue_position} of {job.total_in_queue} in queue
                </span>
              )}
              {job.status === "PENDING" && job.queue_position == null && (
                <RotateCw size={14} className="text-muted-foreground" />
              )}
              {job.status === "RUNNING" && (
                <div className="flex-1 flex items-center gap-2 min-w-0">
                  <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
                    <div
                      className="h-full bg-info rounded-full transition-[width] duration-300"
                      style={{ width: `${job.progress_percentage}%` }}
                    />
                  </div>
                  <span className="text-xs text-muted-foreground w-8 text-right">
                    {job.progress_percentage}%
                  </span>
                </div>
              )}
            </div>
            {job.message && (
              <p className="text-xs text-muted-foreground pl-5 truncate" title={job.message}>
                {job.message}
              </p>
            )}
          </div>
        ))}
        {completedJobs.map((job) => {
          const failed = job.status === "FAILED";
          return (
            <div key={job.job_id} className="space-y-1">
              <div className="flex items-center gap-3">
                <span className={`${statusDot} ${failed ? "bg-destructive" : "bg-success"}`} />
                <span className="text-sm text-foreground truncate" title={displayName(job)}>
                  {displayName(job)}
                </span>
                <span className={`text-xs uppercase ${failed ? "text-destructive" : "text-success"}`}>
                  {failed ? "Failed" : "Completed"}
                </span>
              </div>
              {failed && job.message && (
                <p className="text-xs text-destructive pl-5 truncate" title={job.message}>
                  {job.message}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});

export default JobStatusPanel;
