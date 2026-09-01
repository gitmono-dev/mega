import { useMutation, useQueryClient } from '@tanstack/react-query'

import { DeleteApiConversationByCommentIdData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function useDeleteIssueComment(id: string, params?: RequestParams) {
  const queryClient = useQueryClient()

  return useMutation<DeleteApiConversationByCommentIdData, Error, string>({
    mutationKey: legacyApiClient.v1.deleteApiConversationByCommentId().baseKey,
    mutationFn: (convId) => legacyApiClient.v1.deleteApiConversationByCommentId().request(convId, params),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: legacyApiClient.v1.getApiIssueDetail().requestKey(id)
      })
    }
  })
}
