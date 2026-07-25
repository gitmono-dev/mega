import { useQuery } from '@tanstack/react-query'

import { GetApiTreeParams, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetTree(params: GetApiTreeParams, requestParams?: RequestParams) {
  const finalParams: GetApiTreeParams = { path: params.path }

  if ((params as any).refs) (finalParams as any).refs = (params as any).refs

  return useQuery({
    queryKey: legacyApiClient.v1.getApiTree().requestKey(finalParams),
    queryFn: () => legacyApiClient.v1.getApiTree().request(finalParams, requestParams),
    enabled: !!finalParams.path
  })
}
