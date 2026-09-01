import { useMutation, useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'

import type { DeleteApiAdminGroupsByGroupIdData } from '@gitmono/types'

import { apiErrorToast } from '@/utils/apiErrorToast'
import { legacyApiClient } from '@/utils/queryClient'

export function useDeleteAdminGroup() {
  const queryClient = useQueryClient()

  return useMutation<DeleteApiAdminGroupsByGroupIdData, Error, string>({
    mutationFn: (groupId: string) => legacyApiClient.v1.deleteApiAdminGroupsByGroupId().request(groupId),
    onSuccess: () => {
      // Refresh user groups list
      queryClient.invalidateQueries({
        queryKey: legacyApiClient.v1.postApiAdminGroupsList().baseKey
      })
      toast.success('Group deleted successfully')
    },
    onError: apiErrorToast
  })
}
