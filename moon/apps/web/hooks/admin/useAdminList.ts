import { useQuery } from '@tanstack/react-query'

import type { GetApiAdminListData, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useAdminList(params?: RequestParams & { enabled?: boolean }) {
  const { enabled = true, ...requestParams } = params ?? {}

  return useQuery<GetApiAdminListData, Error>({
    queryKey: [...legacyApiClient.v1.getApiAdminList().requestKey(), requestParams],
    queryFn: () => legacyApiClient.v1.getApiAdminList().request(requestParams),
    staleTime: 0,
    retry: false,
    enabled
  })
}
