import { useQuery } from '@tanstack/react-query'

import type { RunnerListResponse } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetRunnerList(enabled = true) {
  const query = legacyApiClient.v1.getApiOrionRunners()

  return useQuery<RunnerListResponse, Error>({
    queryKey: query.requestKey(),
    queryFn: async () => {
      const result = await query.request()

      if (!result.req_result || !result.data) {
        throw new Error(result.err_message || 'Failed to fetch runner VMs')
      }
      return result.data
    },
    enabled,
    refetchInterval: enabled ? 10_000 : false,
    staleTime: 0
  })
}
