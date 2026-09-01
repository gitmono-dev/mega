import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { PutApiAdminGroupsByGroupIdData, RequestParams, UpdateGroupRequest } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useUpdateAdminGroup() {
  const queryClient = useQueryClient()

  return useMutation<
    PutApiAdminGroupsByGroupIdData,
    Error,
    { groupId: string; data: UpdateGroupRequest; params?: RequestParams }
  >({
    mutationFn: async ({ groupId, data, params }) => {
      const response = await legacyApiClient.v1.putApiAdminGroupsByGroupId().request(groupId, data, params)

      if (response && typeof response === 'object' && 'req_result' in response && !response.req_result) {
        throw new Error(response.err_message || 'Update failed')
      }

      return response
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: legacyApiClient.v1.postApiAdminGroupsList().baseKey
      })
    }
  })
}
