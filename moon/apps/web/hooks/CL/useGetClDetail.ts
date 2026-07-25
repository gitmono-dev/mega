import { useQuery } from '@tanstack/react-query'

import type { RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetClDetail(id: string, params?: RequestParams) {
  return useQuery({
    queryKey: legacyApiClient.v1.getApiClDetail().requestKey(id),
    queryFn: () => legacyApiClient.v1.getApiClDetail().request(id, params),
    enabled: !!id
  })
}
