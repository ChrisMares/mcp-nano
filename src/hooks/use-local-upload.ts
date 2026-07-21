import { useCallback, useMemo, useState } from "react";
import { type FileError, type FileRejection, useDropzone } from "react-dropzone";
import { uploadRepoZip, uploadDocuments, uploadCodeFiles } from "@/utils/apicalls";
import type { EmbeddingOptions } from "@/types/embed";

interface FileWithPreview extends File {
  preview?: string;
  errors: readonly FileError[];
}

type UseLocalUploadOptions = {
  collection: "codebase" | "general";
  codeUploadMode: "zip" | "individual" | "";
  repoName?: string;
  groupName?: string;
  maxFileSize?: number;
  maxFiles?: number;
  onSuccess?: (submittedCount: number) => void | Promise<void>;
};

export type UseLocalUploadReturn = ReturnType<typeof useLocalUpload>;

export const useLocalUpload = (options: UseLocalUploadOptions) => {
  const {
    collection,
    codeUploadMode,
    repoName = "",
    groupName = "",
    maxFileSize = 1000 * 1000 * 200,
    maxFiles = 10,
    onSuccess,
  } = options;

  const [files, setFiles] = useState<FileWithPreview[]>([]);
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<{ name: string; message: string }[]>([]);
  const [successes, setSuccesses] = useState<string[]>([]);

  const isSuccess = useMemo(
    () => errors.length === 0 && successes.length > 0 && successes.length === files.length,
    [errors.length, successes.length, files.length]
  );

  const onDrop = useCallback(
    (acceptedFiles: File[], fileRejections: FileRejection[]) => {
      const validFiles = acceptedFiles
        .filter((file) => !files.find((x) => x.name === file.name))
        .map((file) => {
          (file as FileWithPreview).preview = URL.createObjectURL(file);
          (file as FileWithPreview).errors = [];
          return file as FileWithPreview;
        });

      const invalidFiles = fileRejections.map(({ file, errors: rejectionErrors }) => {
        (file as FileWithPreview).preview = URL.createObjectURL(file);
        (file as FileWithPreview).errors = rejectionErrors;
        return file as FileWithPreview;
      });

      setFiles([...files, ...validFiles, ...invalidFiles]);
    },
    [files]
  );

  const accept = useMemo(() => {
    if (collection === "codebase" && codeUploadMode === "zip") {
      return {
        "application/zip": [],
        "application/x-zip-compressed": [],
      };
    }
    return undefined;
  }, [collection, codeUploadMode]);

  const dropzoneProps = useDropzone({
    onDrop,
    noClick: true,
    accept,
    maxSize: maxFileSize,
    maxFiles,
    multiple: maxFiles !== 1,
  });

  const onUpload = useCallback(async () => {
    setLoading(true);
    try {
      const validFiles = files.filter((f) => !f.errors || f.errors.length === 0);
      if (validFiles.length === 0) {
        throw new Error("No valid files to upload");
      }

      const embeddingOptions: EmbeddingOptions = collection === "codebase"
        ? {
            collection: "codebase",
            repo_name: codeUploadMode === "individual" ? repoName.trim() : undefined,
            metadata: {},
          }
        : {
            collection: "general",
            group: groupName.trim() || "default",
            metadata: {},
          };
      if (collection === "codebase") {
        if (codeUploadMode === "individual") {
          await uploadCodeFiles(validFiles, embeddingOptions);
        } else {
          await uploadRepoZip(validFiles, embeddingOptions);
        }
      } else {
        await uploadDocuments(validFiles, embeddingOptions);
      }

      const uploaded = validFiles.map((f) => f.name);
      setSuccesses(uploaded);
      setErrors([]);

      if (onSuccess) await onSuccess(uploaded.length);
    } catch (err) {
      const message = err instanceof Error ? err.message : typeof err === "string" ? err : "Upload failed";
      setErrors(files.map((f) => ({ name: f.name, message })));
      setSuccesses([]);
    } finally {
      setLoading(false);
    }
  }, [collection, files, codeUploadMode, repoName, groupName, onSuccess]);

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
    ...dropzoneProps,
  };
};
