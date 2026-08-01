import { useCallback, useEffect, useRef, useState } from "react";
import { getActiveJobs } from "@/utils/apicalls";
import { useJobEvents, type JobProgressEvent } from "@/hooks/use-job-events";
import type { EmbedJob, UploadJobEntry } from "@/types/embed";

const mergeJobFromEvent = (
  prev: EmbedJob | undefined,
  e: JobProgressEvent,
  status: string,
): EmbedJob => ({
  job_id: e.job_id,
  status,
  progress_percentage: e.percentage ?? prev?.progress_percentage ?? 0,
  file_name: e.file_name ?? prev?.file_name ?? null,
  created_at: prev?.created_at ?? new Date().toISOString(),
  message: e.message ?? prev?.message ?? null,
});

/**
 * Tracks embedding jobs across the app: seeds from `get_active_jobs`, then
 * follows the worker's `job_queued` / `job_progress` / `job_finished` /
 * `job_failed` Tauri events. Active jobs move to `completedJobs` on their
 * terminal event.
 */
export function useJobTracking() {
  const [activeJobs, setActiveJobs] = useState<EmbedJob[]>([]);
  const [completedJobs, setCompletedJobs] = useState<EmbedJob[]>([]);
  const trackedJobsRef = useRef<Map<string, EmbedJob>>(new Map());

  useEffect(() => {
    let cancelled = false;
    const fetchInitial = async () => {
      try {
        const res = await getActiveJobs();
        if (cancelled) return;
        const jobs = (res.jobs ?? []).map((j) => ({
          job_id: j.job_id,
          status: j.status,
          progress_percentage: j.progress_percentage,
          file_name: j.file_name,
          created_at: j.created_at,
          message: null,
        }));
        setActiveJobs(jobs);
        trackedJobsRef.current = new Map(jobs.map((j) => [j.job_id, j]));
      } catch {
        // sidecars may not be ready yet
      }
    };
    void fetchInitial();
    return () => {
      cancelled = true;
    };
  }, []);

  const upsertActive = useCallback((job: EmbedJob) => {
    trackedJobsRef.current.set(job.job_id, job);
    setActiveJobs((prev) => {
      const idx = prev.findIndex((j) => j.job_id === job.job_id);
      if (idx < 0) return [...prev, job];
      const next = [...prev];
      next[idx] = job;
      return next;
    });
  }, []);

  const handleTerminal = useCallback((e: JobProgressEvent, status: "COMPLETED" | "FAILED") => {
    const prev = trackedJobsRef.current.get(e.job_id);
    trackedJobsRef.current.delete(e.job_id);
    setActiveJobs((prevJobs) => prevJobs.filter((j) => j.job_id !== e.job_id));
    const done = mergeJobFromEvent(prev, e, status);
    done.progress_percentage = 100;
    setCompletedJobs((prevDone) => {
      if (prevDone.find((j) => j.job_id === e.job_id)) return prevDone;
      return [...prevDone, done];
    });
  }, []);

  useJobEvents({
    onQueued: (e) => {
      upsertActive(mergeJobFromEvent(trackedJobsRef.current.get(e.job_id), e, e.status ?? "PENDING"));
    },
    onProgress: (e) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      const next = mergeJobFromEvent(prev, e, e.status ?? "RUNNING");
      if (
        prev &&
        prev.progress_percentage === next.progress_percentage &&
        prev.status === next.status &&
        prev.message === next.message &&
        prev.file_name === next.file_name
      ) {
        return;
      }
      upsertActive(next);
    },
    onFinished: (e) => handleTerminal(e, "COMPLETED"),
    onFailed: (e) => handleTerminal(e, "FAILED"),
  });

  /** Register jobs returned by an upload command (already PENDING server-side). */
  const trackUploadedJobs = useCallback((jobs: UploadJobEntry[]) => {
    for (const entry of jobs) {
      trackedJobsRef.current.set(entry.job_id, {
        job_id: entry.job_id,
        status: entry.status || "PENDING",
        progress_percentage: 0,
        file_name: entry.filename,
        created_at: new Date().toISOString(),
        message: "Queued",
      });
    }
    setActiveJobs((prev) => {
      const byId = new Map(prev.map((j) => [j.job_id, j]));
      for (const entry of jobs) {
        const existing = byId.get(entry.job_id);
        byId.set(entry.job_id, {
          job_id: entry.job_id,
          status: existing?.status === "RUNNING" ? existing.status : entry.status || "PENDING",
          progress_percentage: existing?.progress_percentage ?? 0,
          file_name: entry.filename || existing?.file_name || null,
          created_at: existing?.created_at ?? new Date().toISOString(),
          message: existing?.message ?? "Queued",
        });
      }
      return Array.from(byId.values());
    });
  }, []);

  /** Register a single freshly queued job (e.g. website embed). */
  const trackQueuedJob = useCallback((job: EmbedJob) => {
    trackedJobsRef.current.set(job.job_id, job);
    setActiveJobs((prev) => (prev.some((j) => j.job_id === job.job_id) ? prev : [...prev, job]));
  }, []);

  return { activeJobs, completedJobs, trackUploadedJobs, trackQueuedJob };
}
