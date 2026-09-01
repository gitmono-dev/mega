import { useQuery } from '@tanstack/react-query'

import type { GetApiAdminGroupsByGroupIdData, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

interface UseGetAdminGroupByIdOptions extends RequestParams {
  enabled?: boolean
}

export function useGetAdminGroupById(groupId: string, options?: UseGetAdminGroupByIdOptions) {
  const { enabled = true, ...params } = options || {}

  return useQuery<GetApiAdminGroupsByGroupIdData, Error>({
    queryKey: [...legacyApiClient.v1.getApiAdminGroupsByGroupId().requestKey(groupId), params],
    queryFn: () => legacyApiClient.v1.getApiAdminGroupsByGroupId().request(groupId, params),
    enabled: enabled && !!groupId,
    staleTime: 0,
    retry: false
  })
}
