import { useEffect, useRef } from "react";
import { listen, type UnlistenFn, type Event } from "@tauri-apps/api/event";

/**
 * Shape of job lifecycle events pushed by the Rust worker
 * (`src-tauri/src/worker/progress.rs`).
 */
export interface JobProgressEvent {
  job_id: string;
  percentage: number;
  message?: string | null;
  file_name?: string | null;
  status?: string;
}

export interface JobEventHandlers {
  onQueued?: (e: JobProgressEvent) => void;
  onProgress?: (e: JobProgressEvent) => void;
  onFinished?: (e: JobProgressEvent) => void;
  onFailed?: (e: JobProgressEvent) => void;
}

/**
 * Subscribe to job lifecycle Tauri events for the component lifetime.
 * Handlers are kept via ref so listeners are not re-subscribed each render.
 */
export function useJobEvents(handlers: JobEventHandlers): void {
  const handlersRef = useRef<JobEventHandlers>(handlers);

  useEffect(() => {
    handlersRef.current = handlers;
  });

  useEffect(() => {
    let unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    const attach = async () => {
      const onQueued = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onQueued?.(ev.payload);
      const onProgress = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onProgress?.(ev.payload);
      const onFinished = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onFinished?.(ev.payload);
      const onFailed = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onFailed?.(ev.payload);

      const subs = await Promise.all([
        listen<JobProgressEvent>("job_queued", onQueued),
        listen<JobProgressEvent>("job_progress", onProgress),
        listen<JobProgressEvent>("job_finished", onFinished),
        listen<JobProgressEvent>("job_failed", onFailed),
      ]);
      if (cancelled) {
        subs.forEach((u) => u());
        return;
      }
      unlisteners = subs;
    };
    void attach();

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);
}
