import { useMutation } from '@tanstack/react-query'

import { ContentPayload, PostApiConversationByCommentIdData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

export function usePostComment() {
  return useMutation<
    PostApiConversationByCommentIdData,
    Error,
    { commentId: string; data: ContentPayload; params?: RequestParams }
  >({
    mutationFn: ({ commentId, data, params }) =>
      legacyApiClient.v1.postApiConversationByCommentId().request(commentId, data, params)
  })
}
