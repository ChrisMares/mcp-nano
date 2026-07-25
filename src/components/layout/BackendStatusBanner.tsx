import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getBackendStatus, type BackendStatus } from "@/utils/apicalls";

const ready = (s: BackendStatus) =>
  s.qdrant_ready && s.db_ready && s.embedders_ready && s.worker_ready;

const BackendStatusBanner: React.FC = () => {
  const [status, setStatus] = useState<BackendStatus | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getBackendStatus()
      .then((s) => { if (!cancelled) setStatus(s); })
      .catch(() => {
        if (!cancelled) {
          setStatus({
            qdrant_ready: false,
            qdrant_error: "backend unreachable",
            http_port: null,
            grpc_port: null,
            db_ready: false,
            embedders_ready: false,
            embedding_device: null,
            worker_ready: false,
          });
        }
      });

    listen<BackendStatus>("backend_status", (e) => {
      setStatus(e.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!status) return null;

  const parts: string[] = [];
  if (!status.qdrant_ready) parts.push("Qdrant");
  if (!status.db_ready) parts.push("SQLite");
  if (!status.embedders_ready) parts.push("embedders");
  if (!status.worker_ready) parts.push("worker");

  if (ready(status)) {
    return null;
  }

  return (
    <div
      className={`px-4 py-2 text-sm border-b ${
        status.qdrant_error
          ? "bg-destructive/15 text-destructive border-destructive/30"
          : "bg-warning/15 text-foreground border-warning/30"
      }`}
      role="status"
    >
      {status.qdrant_error ? (
        <span>Backend error: {status.qdrant_error}</span>
      ) : (
        <span>Starting {parts.join(", ")}…</span>
      )}
    </div>
  );
};

export default BackendStatusBanner;
