import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { DeleteApiCodeReviewThreadByThreadIdData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const deleteThreadMutation = legacyApiClient.v1.deleteApiCodeReviewThreadByThreadId()

/**
 * 删除 Thread 及其所有评论
 * DELETE /code_review/thread/{thread_id}
 */
export function useDeleteThread(link: string) {
  const queryClient = useQueryClient()

  return useMutation<DeleteApiCodeReviewThreadByThreadIdData, Error, { threadId: string; params?: RequestParams }>({
    mutationFn: ({ threadId, params }) => deleteThreadMutation.request(threadId, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.refetchQueries({ queryKey, type: 'active' })
    }
  })
}
