import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'react-hot-toast'

import { MONO_API_URL } from '@gitmono/config'

import type { OrionVmImage } from './useGetOrionImages'

export type RegisterOrionImageRequest = {
  digest: string
  object_key: string
  info_object_key?: string | null
  image_name?: string | null
  built_at?: string | null
  rust?: string | null
  buck2?: string | null
  python?: string | null
  kernel?: string | null
  size_bytes?: number | null
  label?: string | null
}

type Envelope = {
  req_result: boolean
  err_message?: string
  data?: OrionVmImage | null
}

export function useRegisterOrionImage() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (body: RegisterOrionImageRequest) => {
      const base = MONO_API_URL.replace(/\/$/, '')
      const res = await fetch(`${base}/api/v1/orion/images`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body)
      })
      const json = (await res.json()) as Envelope

      if (!res.ok || !json?.req_result) {
        throw new Error(json?.err_message || `Failed to register image (${res.status})`)
      }
      return json.data
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['GET:/api/v1/orion/images'] })
      toast.success('Image uploaded')
    },
    onError: (error: Error) => {
      toast.error(error?.message || 'Failed to register image')
    }
  })
}
