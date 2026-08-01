import { useBackendStatus } from "@/hooks/use-backend-status";
import type { BackendStatus } from "@/utils/apicalls";

const ready = (s: BackendStatus) =>
  s.qdrant_ready && s.db_ready && s.embedders_ready && s.worker_ready;

const BackendStatusBanner: React.FC = () => {
  const status = useBackendStatus();

  if (!status || ready(status)) return null;

  const parts: string[] = [];
  if (!status.qdrant_ready) parts.push("Qdrant");
  if (!status.db_ready) parts.push("SQLite");
  if (!status.embedders_ready) parts.push("embedders");
  if (!status.worker_ready) parts.push("worker");

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
