import { useMutation } from '@tanstack/react-query'
import { toast } from 'react-hot-toast'

import type { StartRunnerResponse } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function usePostStartRunner() {
  const mutation = legacyApiClient.v1.postApiOrionRunners()

  return useMutation<StartRunnerResponse, Error, void>({
    mutationFn: async () => {
      const result = await mutation.request({})
      if (!result.req_result || !result.data) {
        throw new Error(result.err_message || 'Failed to start runner')
      }
      return result.data
    },
    onSuccess: () => {
      toast.success('Runner provisioning started')
    },
    onError: (error) => {
      toast.error(error?.message || 'Failed to start runner')
    }
  })
}
