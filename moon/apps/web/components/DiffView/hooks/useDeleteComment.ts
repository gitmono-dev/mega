import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { DeleteApiCodeReviewCommentByCommentIdData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const deleteCommentMutation = legacyApiClient.v1.deleteApiCodeReviewCommentByCommentId()

/**
 * 删除单条评论
 * DELETE /code_review/comment/{comment_id}
 */
export function useDeleteComment(link: string) {
  const queryClient = useQueryClient()

  return useMutation<DeleteApiCodeReviewCommentByCommentIdData, Error, { commentId: string; params?: RequestParams }>({
    mutationFn: ({ commentId, params }) => deleteCommentMutation.request(commentId, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.refetchQueries({ queryKey, type: 'active' })
    }
  })
}
