import { useMutation, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'

import type { AddMembersRequest, PostApiAdminGroupsMembersData } from '@gitmono/types'

import { apiErrorToast } from '@/utils/apiErrorToast'
import { legacyApiClient } from '@/utils/queryClient'

interface AddMembersParams {
  groupId: string
  usernames: string[]
}

export function useAddAdminGroupMembers() {
  const queryClient = useQueryClient()

  return useMutation<PostApiAdminGroupsMembersData, Error, AddMembersParams>({
    mutationFn: ({ groupId, usernames }: AddMembersParams) => {
      const data: AddMembersRequest = { usernames }

      return legacyApiClient.v1.postApiAdminGroupsMembers().request(groupId, data)
    },
    onSuccess: (_, variables) => {
      // Refresh the member list for this user group
      queryClient.invalidateQueries({
        queryKey: legacyApiClient.v1.postApiAdminGroupsMembersList().requestKey(variables.groupId)
      })
      toast.success('Members added successfully')
    },
    onError: apiErrorToast
  })
}
