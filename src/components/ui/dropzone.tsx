import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { CheckCircle, File, Loader2, Upload, X } from 'lucide-react'
import { type PropsWithChildren, useCallback } from 'react'
import { successIconBox, fileIconBox, fileRow } from '@/styles/classes'
import { DropzoneContext, type DropzoneProps } from '@/contexts/DropzoneContext'
import { useDropzoneContext } from '@/hooks/use-dropzone-context'

const BYTE_UNITS = ['bytes', 'KB', 'MB', 'GB', 'TB'] as const

export const formatBytes = (bytes: number, decimals = 2) => {
  if (bytes === 0) return '0 bytes'
  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${BYTE_UNITS[i]}`
}

const Dropzone = ({
  className,
  children,
  getRootProps,
  getInputProps,
  disableUpload,
  disableReason,
  ...restProps
}: PropsWithChildren<DropzoneProps>) => {
  const isSuccess = restProps.isSuccess
  const isActive = restProps.isDragActive
  const isInvalid =
    (restProps.isDragActive && restProps.isDragReject) ||
    (restProps.errors.length > 0 && !restProps.isSuccess) ||
    restProps.files.some((file) => file.errors.length !== 0)

  return (
    <DropzoneContext.Provider value={{ ...restProps, disableUpload, disableReason }}>
      <div
        {...getRootProps({
          className: cn(
            'border-2 rounded-xl p-8 text-center transition-all duration-300 text-foreground cursor-pointer',
            'bg-gradient-to-br from-card via-card to-muted/50 shadow-lg hover:shadow-xl',
            'border-primary/30 hover:border-primary/60',
            className,
            isSuccess ? 'border-solid' : 'border-dashed',
            isActive && 'border-primary bg-primary/10 scale-[1.02] shadow-primary/30 shadow-xl ring-2 ring-primary/30',
            isInvalid && 'border-destructive bg-destructive/10'
          ),
        })}
      >
        <input {...getInputProps()} />
        {children}
      </div>
    </DropzoneContext.Provider>
  )
}

const DropzoneContent = ({ className }: { className?: string }) => {
  const {
    files,
    setFiles,
    onUpload,
    loading,
    successes,
    setSuccesses,
    errors,
    setErrors,
    maxFileSize,
    maxFiles,
    isSuccess,
    disableUpload,
    disableReason,
  } = useDropzoneContext()

  const exceedMaxFiles = files.length > maxFiles

  const handleRemoveFile = useCallback(
    (fileName: string) => {
      setFiles(files.filter((file) => file.name !== fileName))
    },
    [files, setFiles]
  )

  const handleReset = useCallback(() => {
    setFiles([])
    setSuccesses([])
    setErrors([])
  }, [setFiles, setSuccesses, setErrors])

  if (isSuccess) {
    return (
      <div className={cn('flex flex-col', className)}>
        {files.map((file, idx) => (
          <div key={`${file.name}-${idx}`} className={fileRow}>
            <div className={successIconBox}>
                <CheckCircle size={18} className="text-success" />
            </div>
            <div className="shrink grow flex flex-col items-start truncate">
              <p title={file.name} className="text-sm truncate max-w-full">{file.name}</p>
              <p className="text-xs text-success">Upload complete</p>
            </div>
          </div>
        ))}
        <div className="mt-3">
          <Button variant="outline" onClick={handleReset}>
            Upload More
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className={cn('flex flex-col', className)}>
      {files.map((file, idx) => {
        const fileError = errors.find((e) => e.name === file.name)
        const isUploaded = !!successes.find((e) => e === file.name)

        return (
          <div key={`${file.name}-${idx}`} className={fileRow}>
            {isUploaded ? (
              <div className={successIconBox}>
              <CheckCircle size={18} className="text-success" />
              </div>
            ) : file.type.startsWith('image/') ? (
              <div className={`${fileIconBox} overflow-hidden`}>
                <img src={file.preview} alt={file.name} className="object-cover" />
              </div>
            ) : (
              <div className={fileIconBox}>
                <File size={18} />
              </div>
            )}

            <div className="shrink grow flex flex-col items-start truncate">
              <p title={file.name} className="text-sm truncate max-w-full">
                {file.name}
              </p>
              {file.errors.length > 0 ? (
                <p className="text-xs text-destructive">
                  {file.errors
                    .map((e) =>
                      e.message.startsWith('File is larger than')
                        ? `File is larger than ${formatBytes(maxFileSize, 2)} (Size: ${formatBytes(file.size, 2)})`
                        : e.message
                    )
                    .join(', ')}
                </p>
              ) : loading && !isUploaded ? (
                <p className="text-xs text-muted-foreground">Uploading file...</p>
              ) : fileError ? (
                <p className="text-xs text-destructive">Failed to upload: {fileError.message}</p>
              ) : isUploaded ? (
                <p className="text-xs text-primary">Successfully uploaded file</p>
              ) : (
                <p className="text-xs text-muted-foreground">{formatBytes(file.size, 2)}</p>
              )}
            </div>

            {!loading && !isUploaded && (
              <Button
                size="icon"
                variant="link"
                className="shrink-0 justify-self-end text-muted-foreground hover:text-foreground"
                onClick={() => handleRemoveFile(file.name)}
              >
                <X />
              </Button>
            )}
          </div>
        )
      })}
      {exceedMaxFiles && (
        <p className="text-sm text-left mt-2 text-destructive">
          You may upload only up to {maxFiles} files, please remove {files.length - maxFiles} file
          {files.length - maxFiles > 1 ? 's' : ''}.
        </p>
      )}
      {files.length > 0 && !exceedMaxFiles && (
        <div className="mt-2">
          {disableUpload && disableReason && (
            <p className="text-sm text-destructive mb-2">{disableReason}</p>
          )}
          <Button
            variant="outline"
            onClick={onUpload}
            disabled={files.some((file) => file.errors.length !== 0) || loading || disableUpload}
          >
            {loading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                Uploading...
              </>
            ) : (
              <>Upload files</>
            )}
          </Button>
        </div>
      )}
    </div>
  )
}

const DropzoneEmptyState = ({ className }: { className?: string }) => {
  const { maxFiles, maxFileSize, inputRef, isSuccess } = useDropzoneContext()

  if (isSuccess) {
    return null
  }

  return (
    <div className={cn('flex flex-col items-center gap-y-3', className)}>
      <div className="p-4 rounded-full bg-primary/15 ring-1 ring-primary/30">
        <Upload size={28} className="text-primary" />
      </div>
      <p className="text-base font-semibold">
        Upload{!!maxFiles && maxFiles > 1 ? ` ${maxFiles}` : ''} file
        {!maxFiles || maxFiles > 1 ? 's' : ''}
      </p>
      <div className="flex flex-col items-center gap-y-1.5">
        <p className="text-sm text-muted-foreground">
          Drag and drop or{' '}
          <a
            onClick={() => inputRef.current?.click()}
            className="underline cursor-pointer text-primary hover:text-primary/80 font-medium"
          >
            select {maxFiles === 1 ? `file` : 'files'}
          </a>{' '}
          to upload
        </p>
        {maxFileSize !== Number.POSITIVE_INFINITY && (
          <p className="text-xs text-muted-foreground/70">
            Maximum file size: {formatBytes(maxFileSize, 2)}
          </p>
        )}
      </div>
    </div>
  )
}

export { Dropzone, DropzoneContent, DropzoneEmptyState }
