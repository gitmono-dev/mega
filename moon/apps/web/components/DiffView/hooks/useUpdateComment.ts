import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { PostApiCodeReviewUpdateData, RequestParams, UpdateCommentRequest } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const updateCommentMutation = legacyApiClient.v1.postApiCodeReviewUpdate()

/**
 * 更新指定评论内容
 * POST /code_review/{comment_id}/update
 */
export function useUpdateComment(link: string) {
  const queryClient = useQueryClient()

  return useMutation<
    PostApiCodeReviewUpdateData,
    Error,
    { commentId: string; data: UpdateCommentRequest; params?: RequestParams }
  >({
    mutationFn: ({ commentId, data, params }) => updateCommentMutation.request(commentId, data, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.refetchQueries({ queryKey, type: 'active' })
    }
  })
}
