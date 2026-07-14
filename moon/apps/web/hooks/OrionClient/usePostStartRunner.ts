import { useMutation } from '@tanstack/react-query'
import { toast } from 'react-hot-toast'

import type { StartRunnerRequest, StartRunnerResponse } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function usePostStartRunner() {
  const mutation = legacyApiClient.v1.postApiOrionRunners()

  return useMutation<StartRunnerResponse, Error, StartRunnerRequest | void>({
    mutationFn: async (body) => {
      const result = await mutation.request(body ?? {})
      if (!result.req_result || !result.data) {
        throw new Error(result.err_message || 'Failed to start runner')
      }
      return result.data
    },
    onSuccess: (data) => {
      if (data.phase === 'running') {
        toast.success(data.domain ? `Runner already running (${data.domain})` : 'Runner already running')
      } else {
        toast.success(data.domain ? `Runner provisioning started (${data.domain})` : 'Runner provisioning started')
      }
    },
    onError: (error) => {
      toast.error(error?.message || 'Failed to start runner')
    }
  })
}
