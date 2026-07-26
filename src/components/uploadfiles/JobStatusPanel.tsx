import React from "react";
import { Loader2 } from "lucide-react";
import type { EmbedJob } from "@/types/embed";
import JobStatusRow from "./JobStatusRow";
import JobStatusCompleteRow from "./JobStatusCompleteRow";

interface Props {
  processing: boolean;
  activeJobs: EmbedJob[];
  completedJobs: EmbedJob[];
}

const JobStatusPanel: React.FC<Props> = React.memo(
  ({ processing, activeJobs, completedJobs }) => {
    const hasVisibleJobs =
      processing || activeJobs.length > 0 || completedJobs.length > 0;
    if (!hasVisibleJobs) return null;

    return (
      <div className="mt-8 p-4 rounded-lg border border-border bg-card">
        <h3 className="text-sm font-medium text-foreground mb-3">
          Embedding Status
        </h3>
        <p className="text-muted-foreground mb-6 text-sm">
          You may navigate away from this page, or upload more files. Your
          embedding tasks are queued up.
        </p>
        <div className="space-y-3">
          {processing &&
            activeJobs.length === 0 &&
            completedJobs.length === 0 && (
              <div className="flex items-center gap-3">
                <Loader2 size={14} className="animate-spin text-info" />
                <span className="text-sm text-muted-foreground">
                  Submitting files for processing...
                </span>
              </div>
            )}
          {activeJobs.map((job) => (
            <JobStatusRow key={job.job_id} embedJob={job} />
          ))}
          {completedJobs.map((job) => (
            <JobStatusCompleteRow key={job.job_id} embedJob={job} />
          ))}
        </div>
      </div>
    );
  },
);

export default JobStatusPanel;
