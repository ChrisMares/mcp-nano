import React, { useEffect, useState } from "react";
import { EmbedJob } from "@/types/embed";
import { RotateCw, X } from "lucide-react";
import {
  btnCancel,
  btnConfirm,
  btnIconDelete,
  modalOverlay,
  modalPanel,
  statusDot,
} from "@/styles/classes";
import { cancelJob } from "@/utils/apicalls";

interface Props {
  embedJob: EmbedJob;
}

const JobStatusRow: React.FC<Props> = ({ embedJob }) => {
  const [percentComplete, setPercentComplete] = useState<number>(
    embedJob.progress_percentage || 0,
  );
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [cancelError, setCancelError] = useState<string | null>(null);

  useEffect(() => {
    const incoming = embedJob.progress_percentage ?? 0;

    // Progress bars only move forward; stale events can't rewind them.
    setPercentComplete((prev) => (incoming > prev ? incoming : prev));
  }, [embedJob.progress_percentage]);

  const displayName = embedJob.file_name?.trim() || "Untitled";
  const isActive =
    embedJob.status === "PENDING" || embedJob.status === "RUNNING";

  const handleConfirmCancel = async () => {
    setCancelRequested(true);
    setCancelError(null);
    try {
      await cancelJob(embedJob.job_id);
      setConfirmOpen(false);
    } catch (e) {
      setCancelRequested(false);
      setCancelError(e instanceof Error ? e.message : String(e));
    }
  };

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
        {isActive && (
          <button
            type="button"
            onClick={() => setConfirmOpen(true)}
            disabled={cancelRequested}
            className={btnIconDelete}
            title="Cancel this job"
            aria-label={`Cancel ${displayName}`}
          >
            <X />
          </button>
        )}
      </div>
      {cancelRequested ? (
        <p className="text-xs text-muted-foreground pl-5 truncate">
          Cancelling...
        </p>
      ) : (
        embedJob.message && (
          <p
            className="text-xs text-muted-foreground pl-5 truncate"
            title={embedJob.message}
          >
            {embedJob.message}
          </p>
        )
      )}

      {confirmOpen && (
        <div className={modalOverlay}>
          <div className={modalPanel}>
            <h3 className="text-lg font-semibold text-foreground mb-2">
              Cancel job
            </h3>
            <p className="text-sm text-muted-foreground mb-4">
              Cancel{" "}
              <span className="font-medium text-foreground">{displayName}</span>?
              Anything this run has already embedded will be deleted.
            </p>
            {cancelError && (
              <p className="text-xs text-destructive mb-3">{cancelError}</p>
            )}
            <div className="flex justify-end gap-3">
              <button
                type="button"
                onClick={() => setConfirmOpen(false)}
                className={btnCancel}
              >
                Keep running
              </button>
              <button
                type="button"
                onClick={handleConfirmCancel}
                disabled={cancelRequested}
                className={btnConfirm}
              >
                {cancelRequested ? "Cancelling..." : "Cancel job"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default JobStatusRow;
