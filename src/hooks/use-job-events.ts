import { useEffect, useRef } from "react";
import { listen, type UnlistenFn, type Event } from "@tauri-apps/api/event";

/**
 * Shape of the `job_progress` / `job_finished` / `job_failed` events pushed
 * by the Rust worker (`src-tauri/src/worker/progress.rs` and
 * `src-tauri/src/worker/mod.rs::run_job`).
 *
 * Replaces the previous `getJobStatus` polling loop — the UI subscribes to
 * the three events once at startup and updates job state imperatively as
 * events arrive from the worker.
 */
export interface JobProgressEvent {
  job_id: string;
  percentage: number;
  message?: string;
}

export interface JobEventHandlers {
  /** Called for every `job_progress` event. */
  onProgress?: (e: JobProgressEvent) => void;
  /** Called once per job when the worker sets status=FINISHED. */
  onFinished?: (e: JobProgressEvent) => void;
  /** Called once per job when the worker sets status=FAILED. */
  onFailed?: (e: JobProgressEvent) => void;
}

/**
 * Subscribe to the three job lifecycle Tauri events for the lifetime of the
 * component. Returns nothing — handlers are invoked imperatively from
 * inside the listeners, so React state set inside the callbacks triggers
 * re-renders as usual.
 *
 * Important: the `handlers` object is captured at mount time only; updates
 * to it on re-render are intentionally ignored (closure identity is not
 * part of the dependency graph) to keep event-listener churn at zero. If
 * you need fresh state inside a handler, lift the state up via refs.
 *
 * Example:
 *   useJobEvents({
 *     onProgress: (e) => setActive(prev => update(prev, e)),
 *     onFinished: (e) => setCompleted(prev => add(prev, e)),
 *     onFailed: (e) => setCompleted(prev => add(prev, { ...e, status: "FAILED" })),
 *   });
 */
export function useJobEvents(handlers: JobEventHandlers): void {
  // Keep a stable ref so the listeners see the latest callbacks without
  // re-subscribing. Equivalent to the React `useEvent` proposal from the
  // working group: stable listener identity, fresh handler instance. The
  // assignment happens inside `useEffect` (not during render) so React's
  // strict-mode ref-warning lint pass doesn't complain.
  const handlersRef = useRef<JobEventHandlers>(handlers);

  useEffect(() => {
    handlersRef.current = handlers;
  });

  useEffect(() => {
    let unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    const attach = async () => {
      const onProgress = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onProgress?.(ev.payload);
      const onFinished = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onFinished?.(ev.payload);
      const onFailed = (ev: Event<JobProgressEvent>) =>
        handlersRef.current.onFailed?.(ev.payload);

      const subs = await Promise.all([
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