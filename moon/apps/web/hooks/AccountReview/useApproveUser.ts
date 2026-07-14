import { useMutation, useQueryClient } from '@tanstack/react-query'

import { apiErrorToast } from '@/utils/apiErrorToast'
import { legacyApiClient } from '@/utils/queryClient'

export function useApproveUser() {
  const queryClient = useQueryClient()
  const api = legacyApiClient.v1.postApiAdminUserApprovalsApprove()

  return useMutation({
    mutationFn: (username: string) => api.request(username),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: legacyApiClient.v1.getApiAdminUserApprovals().baseKey }),
        queryClient.invalidateQueries({ queryKey: legacyApiClient.v1.getApiUserApprovalStatus().baseKey })
      ])
    },
    onError: apiErrorToast
  })
}
