import { useQuery } from '@tanstack/react-query'

import type { GetApiUserApprovalStatusData } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useMyApprovalStatus(enabled = true) {
  const api = legacyApiClient.v1.getApiUserApprovalStatus()

  return useQuery<GetApiUserApprovalStatusData, Error>({
    queryKey: api.requestKey(),
    queryFn: () => api.request(),
    enabled,
    staleTime: 0,
    retry: false
  })
}
