import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { CommentReplyRequest, PostApiCodeReviewCommentReplyData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const replyCommentMutation = legacyApiClient.v1.postApiCodeReviewCommentReply()

/**
 * 回复指定 Thread 的评论
 * POST /code_review/{thread_id}/comment/reply
 */
export function useReplyComment(link: string) {
  const queryClient = useQueryClient()

  return useMutation<
    PostApiCodeReviewCommentReplyData,
    Error,
    { threadId: string; data: CommentReplyRequest; params?: RequestParams }
  >({
    mutationFn: ({ threadId, data, params }) => replyCommentMutation.request(threadId, data, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.refetchQueries({ queryKey, type: 'active' })
    }
  })
}
