import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'react-hot-toast'

import { MONO_API_URL } from '@gitmono/config'

import type { OrionVmImage } from './useGetOrionImages'

type DeleteEnvelope = {
  req_result: boolean
  err_message?: string
  data?: OrionVmImage | null
}

export function useDeleteOrionImage() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (id: string) => {
      const base = MONO_API_URL.replace(/\/$/, '')
      const res = await fetch(`${base}/api/v1/orion/images/${encodeURIComponent(id)}`, {
        method: 'DELETE',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' }
      })
      const body = (await res.json()) as DeleteEnvelope

      if (!res.ok || !body?.req_result) {
        throw new Error(body?.err_message || `Failed to delete image (${res.status})`)
      }
      return body.data
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['GET:/api/v1/orion/images'] })
      toast.success('Image deleted')
    },
    onError: (error: Error) => {
      toast.error(error?.message || 'Failed to delete image')
    }
  })
}
