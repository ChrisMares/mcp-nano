/** Settings page: application version, model/device status, and local storage locations. */
import PageHead from "@/components/shared/PageHead";
import { useBackendStatus } from "@/hooks/use-backend-status";
import { card } from "@/styles/classes";
import { formatBytes } from "@/utils/format-bytes";
import packageJson from "../../package.json";

const deviceClass = (device: string) =>
  device.includes("GPU")
    ? "bg-success/15 text-success"
    : "bg-muted text-muted-foreground";

const pillClass =
  "ml-auto rounded-full bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground";

interface StorageLocation {
  name: string;
  path: string | null;
  size?: number | null;
}

/** Shows version, model status, and storage paths. Version and storage are always shown, even when models failed to load. */
const Settings: React.FC = () => {
  const status = useBackendStatus();
  const models = status?.model_statuses ?? [];
  const storageLocations: StorageLocation[] = [
    { name: "Qdrant", path: status?.qdrant_storage_path ?? null },
    { name: "SQLite", path: status?.sqlite_path ?? null },
    { name: "Logs", path: status?.logs_path ?? null, size: status?.logs_size_bytes ?? null },
  ];

  let modelMessage: string | null = null;
  if (!status) {
    modelMessage = "Loading model status...";
  } else if (models.length === 0) {
    modelMessage =
      "Model status is unavailable because the embedding models have not loaded.";
  }

  return (
    <div className="space-y-6">
      <PageHead
        title="Settings"
        description="View the local models and devices used for search."
      />

      <h1 className="text-2xl font-bold text-foreground">Settings</h1>

      <section className={card} aria-labelledby="model-status-heading">
        <h2
          id="model-status-heading"
          className="text-lg font-semibold text-foreground mb-2"
        >
          Model Status
        </h2>
        <ul className="divide-y divide-border">
          <li className="py-4 first:pt-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium text-foreground">
                Application Version
              </span>
              <span className={pillClass}>{packageJson.version}</span>
            </div>
          </li>
          {modelMessage && (
            <li className="py-4">
              <p className="text-sm text-muted-foreground">{modelMessage}</p>
            </li>
          )}
          {models.map((model) => (
            <li key={model.role} className="py-4">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-foreground">
                  {model.role}
                </span>
                <span className="text-sm text-muted-foreground">
                  {model.model}
                </span>
                <span
                  className={`ml-auto rounded-full px-2 py-0.5 text-xs font-medium ${deviceClass(model.device)}`}
                >
                  {model.device}
                </span>
              </div>
              {model.cpu_reason && (
                <p className="mt-2 text-sm text-muted-foreground">
                  {model.cpu_reason}
                </p>
              )}
            </li>
          ))}
          {storageLocations.map((location) => (
            <li key={location.name} className="py-4 last:pb-0">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-foreground">
                  {location.name}
                </span>
                <span className="text-sm text-muted-foreground">
                  Local storage
                </span>
                {location.size !== undefined && (
                  <span className={pillClass}>
                    {formatBytes(location.size, "Size unavailable")}
                  </span>
                )}
              </div>
              <p className="mt-2 break-all font-mono text-sm text-muted-foreground">
                {location.path ?? "Location unavailable"}
              </p>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
};

export default Settings;
