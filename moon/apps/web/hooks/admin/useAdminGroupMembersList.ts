import { useQuery } from '@tanstack/react-query'

import type { PageParamsEmptyListAdditional, PostApiAdminGroupsMembersListData, RequestParams } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useAdminGroupMembersList(groupId: string, data: PageParamsEmptyListAdditional, params?: RequestParams) {
  return useQuery<PostApiAdminGroupsMembersListData, Error>({
    queryKey: [...legacyApiClient.v1.postApiAdminGroupsMembersList().requestKey(groupId), data, params],
    queryFn: () => legacyApiClient.v1.postApiAdminGroupsMembersList().request(groupId, data, params),
    enabled: !!groupId,
    staleTime: 0,
    retry: false
  })
}
