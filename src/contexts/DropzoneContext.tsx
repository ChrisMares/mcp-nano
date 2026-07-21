import { createContext } from 'react'
import { type UseLocalUploadReturn } from '@/hooks/use-local-upload'

export type DropzoneContextType = Omit<UseLocalUploadReturn, 'getRootProps' | 'getInputProps'> & {
  disableUpload?: boolean
  disableReason?: string
}

export type DropzoneProps = UseLocalUploadReturn & {
  className?: string
  disableUpload?: boolean
  disableReason?: string
}

export const DropzoneContext = createContext<DropzoneContextType | undefined>(undefined)
