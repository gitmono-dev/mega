import { useQuery } from '@tanstack/react-query'

import type { RunnerStatusResponse } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetRunnerStatus(vmId: string | null, phase: string | null) {
  const shouldPoll = !!vmId && phase === 'provisioning'
  const query = legacyApiClient.v1.getApiOrionRunnersById()

  return useQuery<RunnerStatusResponse, Error>({
    queryKey: vmId ? query.requestKey(vmId) : query.baseKey,
    queryFn: async () => {
      const result = await query.request(vmId!)
      if (!result.req_result || !result.data) {
        throw new Error(result.err_message || 'Failed to fetch runner status')
      }
      return result.data
    },
    enabled: !!vmId,
    refetchInterval: shouldPoll ? 5000 : false,
    staleTime: 0
  })
}
