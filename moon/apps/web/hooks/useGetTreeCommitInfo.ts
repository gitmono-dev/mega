import { useQuery } from '@tanstack/react-query'

import { GetApiTreeCommitInfoParams, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

const treeCommitInfoQuery = legacyApiClient.v1.getApiTreeCommitInfo()

export function useGetTreeCommitInfo(path: string, refs?: string, requestParams?: RequestParams) {
  const params: GetApiTreeCommitInfoParams = { path }

  if (refs) params.refs = refs

  return useQuery({
    queryKey: treeCommitInfoQuery.requestKey(params),
    queryFn: () => treeCommitInfoQuery.request(params, requestParams)
  })
}
