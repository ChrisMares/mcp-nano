import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getBackendStatus, type BackendStatus } from "@/utils/apicalls";

const unreachableStatus: BackendStatus = {
  qdrant_ready: false,
  qdrant_error: "backend unreachable",
  http_port: null,
  grpc_port: null,
  db_ready: false,
  embedders_ready: false,
  embedding_device: null,
  worker_ready: false,
};

/**
 * Current backend readiness: fetched once, then follows `backend_status`
 * Tauri events pushed by Rust while sidecars come online.
 */
export function useBackendStatus(): BackendStatus | null {
  const [status, setStatus] = useState<BackendStatus | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getBackendStatus()
      .then((s) => { if (!cancelled) setStatus(s); })
      .catch(() => { if (!cancelled) setStatus(unreachableStatus); });

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

  return status;
}
