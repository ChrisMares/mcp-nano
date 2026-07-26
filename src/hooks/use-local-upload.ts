import { useCallback, useEffect, useMemo, useRef, useState, type HTMLAttributes, type MouseEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  uploadRepoZip,
  uploadDocuments,
  uploadCodeFiles,
} from "@/utils/apicalls";
import type { EmbeddingOptions, UploadJobEntry } from "@/types/embed";

type FileError = {
  code: string;
  message: string;
};

export interface LocalUploadFile {
  name: string;
  path: string;
  size: number;
  type: string;
  errors: readonly FileError[];
  preview?: string;
}

type UseLocalUploadOptions = {
  collection: "codebase" | "general";
  codeUploadMode: "zip" | "individual" | "";
  repoName?: string;
  groupName?: string;
  maxFileSize?: number;
  maxFiles?: number;
  onSuccess?: (submittedCount: number, jobs: UploadJobEntry[]) => void | Promise<void>;
};

export type UseLocalUploadReturn = ReturnType<typeof useLocalUpload>;

const fileName = (path: string) => path.replace(/\\/g, "/").split("/").pop() || path;

const mimeType = (path: string) => {
  const extension = path.toLowerCase().split(".").pop();
  if (extension === "zip") return "application/zip";
  if (["png", "jpg", "jpeg", "gif", "webp"].includes(extension ?? "")) {
    return `image/${extension === "jpg" ? "jpeg" : extension}`;
  }
  return "application/octet-stream";
};

export const useLocalUpload = (options: UseLocalUploadOptions) => {
  const {
    collection,
    codeUploadMode,
    repoName = "",
    groupName = "",
    maxFileSize = Number.POSITIVE_INFINITY,
    maxFiles = 10,
    onSuccess,
  } = options;

  const [files, setFiles] = useState<LocalUploadFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<{ name: string; message: string }[]>([]);
  const [successes, setSuccesses] = useState<string[]>([]);
  const [isDragActive, setIsDragActive] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const isDragActiveRef = useRef(false);

  const acceptsZip = collection === "codebase" && codeUploadMode === "zip";

  const isOverDropzone = useCallback((position: { x: number; y: number }) => {
    const bounds = rootRef.current?.getBoundingClientRect();
    if (!bounds) return false;
    const scale = window.devicePixelRatio;
    const x = position.x / scale;
    const y = position.y / scale;
    return x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
  }, []);

  const setDragActive = useCallback((active: boolean) => {
    if (isDragActiveRef.current === active) return;
    isDragActiveRef.current = active;
    setIsDragActive(active);
  }, []);

  const addPaths = useCallback(
    (paths: string[]) => {
      setFiles((current) => {
        const existingPaths = new Set(current.map((file) => file.path));
        const available = Math.max(0, maxFiles - current.length);
        const newFiles = paths
          .filter((path) => !existingPaths.has(path))
          .slice(0, available)
          .map((path): LocalUploadFile => {
            const name = fileName(path);
            const isZip = name.toLowerCase().endsWith(".zip");
            const fileErrors: FileError[] = acceptsZip && !isZip
              ? [{ code: "file-invalid-type", message: "Only .zip files are accepted" }]
              : [];
            return { name, path, size: 0, type: mimeType(path), errors: fileErrors };
          });
        return [...current, ...newFiles];
      });
    },
    [acceptsZip, maxFiles],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setDragActive(isOverDropzone(event.payload.position));
        } else if (event.payload.type === "leave") {
          setDragActive(false);
        } else if (isOverDropzone(event.payload.position)) {
          setDragActive(false);
          addPaths(event.payload.paths);
        } else {
          setDragActive(false);
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      unlisten?.();
    };
  }, [addPaths, isOverDropzone, setDragActive]);

  const isSuccess = useMemo(
    () => errors.length === 0 && successes.length > 0 && successes.length === files.length,
    [errors.length, successes.length, files.length],
  );

  const selectFiles = useCallback(async () => {
    const selected = await open({
      multiple: maxFiles !== 1,
      directory: false,
      filters: acceptsZip ? [{ name: "ZIP archives", extensions: ["zip"] }] : undefined,
    });
    if (!selected) return;
    addPaths(Array.isArray(selected) ? selected : [selected]);
  }, [acceptsZip, addPaths, maxFiles]);

  const getRootProps = useCallback(
    (props: HTMLAttributes<HTMLDivElement> = {}) => ({
      ...props,
      ref: rootRef,
      onClick: (event: MouseEvent<HTMLDivElement>) => {
        props.onClick?.(event);
        if (event.defaultPrevented) return;
        const target = event.target as HTMLElement | null;
        if (target?.closest("button, a, input, label, [role='button']")) return;
        void selectFiles();
      },
    }),
    [selectFiles],
  );

  const getInputProps = useCallback(() => ({ type: "file", hidden: true }), []);

  const onUpload = useCallback(async () => {
    setLoading(true);
    try {
      const validFiles = files.filter((file) => file.errors.length === 0);
      if (validFiles.length === 0) {
        throw new Error("No valid files to upload");
      }

      const embeddingOptions: EmbeddingOptions = collection === "codebase"
        ? {
            collection: "codebase",
            // Zip mode: backend defaults repo_name to zip basename minus ".zip"
            // when this is empty. Individual mode uses the user-entered name.
            repo_name:
              codeUploadMode === "individual"
                ? repoName.trim() || undefined
                : undefined,
            metadata: {},
          }
        : {
            collection: "general",
            group: groupName.trim() || "default",
            metadata: {},
          };
      const paths = validFiles.map((file) => file.path);
      const response = collection === "codebase"
        ? codeUploadMode === "individual"
          ? await uploadCodeFiles(paths, embeddingOptions)
          : await uploadRepoZip(paths, embeddingOptions)
        : await uploadDocuments(paths, embeddingOptions);

      if (response.errors.length > 0) {
        throw new Error(response.errors.join("; "));
      }

      const uploaded = validFiles.map((file) => file.name);
      setSuccesses(uploaded);
      setErrors([]);
      if (onSuccess) await onSuccess(uploaded.length, response.jobs ?? []);
    } catch (error) {
      const message = error instanceof Error ? error.message : typeof error === "string" ? error : "Upload failed";
      setErrors(files.map((file) => ({ name: file.name, message })));
      setSuccesses([]);
    } finally {
      setLoading(false);
    }
  }, [codeUploadMode, collection, files, groupName, onSuccess, repoName]);

  return {
    files,
    setFiles,
    successes,
    setSuccesses,
    isSuccess,
    loading,
    errors,
    setErrors,
    onUpload,
    maxFileSize,
    maxFiles,
    allowedMimeTypes: [],
    isDragActive,
    isDragAccept: isDragActive,
    isDragReject: false,
    getRootProps,
    getInputProps,
    inputRef: { current: { click: selectFiles } },
    rootRef,
  };
};
