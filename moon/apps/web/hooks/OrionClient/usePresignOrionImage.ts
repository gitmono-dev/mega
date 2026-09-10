import { useMutation } from '@tanstack/react-query'

import { MONO_API_URL } from '@gitmono/config'

export type PresignOrionImageRequest = {
  digest: string
  image_name?: string
  with_info?: boolean
}

export type PresignOrionImageResponse = {
  object_key: string
  info_object_key?: string | null
  qcow2_put_url: string
  info_put_url?: string | null
  expires_in_secs: number
}

type Envelope = {
  req_result: boolean
  err_message?: string
  data?: PresignOrionImageResponse | null
}

export function usePresignOrionImage() {
  return useMutation({
    mutationFn: async (body: PresignOrionImageRequest) => {
      const base = MONO_API_URL.replace(/\/$/, '')
      const res = await fetch(`${base}/api/v1/orion/images/presign`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body)
      })
      const text = await res.text()
      let json: Envelope | null = null

      try {
        json = text ? (JSON.parse(text) as Envelope) : null
      } catch {
        throw new Error(text || `Failed to prepare upload (${res.status})`)
      }

      if (!res.ok || !json?.req_result || !json.data) {
        throw new Error(json?.err_message || text || `Failed to prepare upload (${res.status})`)
      }
      return json.data
    }
  })
}
