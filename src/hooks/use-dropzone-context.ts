import { useContext } from 'react'
import { DropzoneContext, type DropzoneContextType } from '@/contexts/DropzoneContext'

export const useDropzoneContext = (): DropzoneContextType => {
  const context = useContext(DropzoneContext)

  if (!context) {
    throw new Error('useDropzoneContext must be used within a Dropzone')
  }

  return context
}
