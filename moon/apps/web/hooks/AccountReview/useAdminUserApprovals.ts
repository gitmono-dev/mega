import { useQuery } from '@tanstack/react-query'

import type { GetApiAdminUserApprovalsData, GetApiAdminUserApprovalsParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useAdminUserApprovals(status: string, enabled = true) {
  const api = legacyApiClient.v1.getApiAdminUserApprovals()
  const query: GetApiAdminUserApprovalsParams = { status }

  return useQuery<GetApiAdminUserApprovalsData, Error>({
    queryKey: api.requestKey(query),
    queryFn: () => api.request(query),
    enabled,
    staleTime: 0,
    retry: false
  })
}
