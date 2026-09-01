import { useMutation, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'

import type { DeleteApiAdminGroupsMembersByUsernameData } from '@gitmono/types'

import { apiErrorToast } from '@/utils/apiErrorToast'
import { legacyApiClient } from '@/utils/queryClient'

interface DeleteMemberParams {
  groupId: string
  username: string
}

export function useDeleteAdminGroupMember() {
  const queryClient = useQueryClient()

  return useMutation<DeleteApiAdminGroupsMembersByUsernameData, Error, DeleteMemberParams>({
    mutationFn: ({ groupId, username }: DeleteMemberParams) => {
      return legacyApiClient.v1.deleteApiAdminGroupsMembersByUsername().request(groupId, username)
    },
    onSuccess: (_, variables) => {
      // Refresh the member list for this user group
      queryClient.invalidateQueries({
        queryKey: legacyApiClient.v1.postApiAdminGroupsMembersList().requestKey(variables.groupId)
      })
      toast.success('Member removed successfully')
    },
    onError: apiErrorToast
  })
}
