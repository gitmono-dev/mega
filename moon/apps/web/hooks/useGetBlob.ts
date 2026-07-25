import { useQuery } from '@tanstack/react-query'

import { GetApiBlobParams, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useGetBlob(params: GetApiBlobParams & { refs?: string }, requestParams?: RequestParams) {
  const finalParams: any = { path: params.path }

  if (params.refs) finalParams.refs = params.refs

  return useQuery({
    queryKey: legacyApiClient.v1.getApiBlob().requestKey(finalParams as GetApiBlobParams),
    queryFn: () => legacyApiClient.v1.getApiBlob().request(finalParams, requestParams),
    enabled: !!params.path
  })
}
