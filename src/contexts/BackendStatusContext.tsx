/**
 * App-wide backend readiness. Fetches `get_backend_status` once and then
 * follows `backend_status` events, so every consumer shares one IPC
 * subscription instead of each component opening its own.
 */
import type React from "react";
import { createContext, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getBackendStatus } from "@/utils/apicalls";
import type { BackendStatus } from "@/types/api";

const unreachableStatus: BackendStatus = {
  qdrant_ready: false,
  qdrant_error: "backend unreachable",
  http_port: null,
  grpc_port: null,
  db_ready: false,
  embedders_ready: false,
  embedding_device: null,
  model_statuses: [],
  qdrant_storage_path: null,
  sqlite_path: null,
  logs_path: null,
  logs_size_bytes: null,
  worker_ready: false,
};

/** `null` until the first status arrives. */
export const BackendStatusContext = createContext<BackendStatus | null>(null);

/** Provides the live {@link BackendStatus} to the component tree. */
export const BackendStatusProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [status, setStatus] = useState<BackendStatus | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getBackendStatus()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch(() => {
        if (!cancelled) setStatus(unreachableStatus);
      });

    listen<BackendStatus>("backend_status", (e) => {
      if (!cancelled) setStatus(e.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return (
    <BackendStatusContext.Provider value={status}>
      {children}
    </BackendStatusContext.Provider>
  );
};
