import { useQuery } from '@tanstack/react-query'

import { GetApiBlameParams, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetBlame(params: GetApiBlameParams, requestParams?: RequestParams) {
  return useQuery({
    queryKey: legacyApiClient.v1.getApiBlame().requestKey(params),
    queryFn: () => legacyApiClient.v1.getApiBlame().request(params, requestParams),
    enabled: !!params.path
  })
}
