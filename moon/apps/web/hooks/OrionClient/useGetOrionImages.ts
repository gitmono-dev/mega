import { useQuery } from '@tanstack/react-query'

import { MONO_API_URL } from '@gitmono/config'

export type OrionVmImage = {
  id: string
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
  created_at: string
}

type ListEnvelope = {
  req_result: boolean
  err_message?: string
  data?: { count: number; images: OrionVmImage[] } | null
}

async function fetchOrionImages(): Promise<OrionVmImage[]> {
  const base = MONO_API_URL.replace(/\/$/, '')
  const res = await fetch(`${base}/api/v1/orion/images`, {
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' }
  })
  const body = (await res.json()) as ListEnvelope

  if (!res.ok || !body?.req_result || !body.data) {
    throw new Error(body?.err_message || `Failed to list Orion images (${res.status})`)
  }
  return body.data.images ?? []
}

export function useGetOrionImages(enabled: boolean) {
  return useQuery({
    queryKey: ['GET:/api/v1/orion/images'],
    enabled,
    queryFn: fetchOrionImages,
    staleTime: 30_000
  })
}
