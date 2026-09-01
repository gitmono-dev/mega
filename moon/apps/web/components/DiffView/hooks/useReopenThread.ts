import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { PostApiCodeReviewReopenData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const reopenThreadMutation = legacyApiClient.v1.postApiCodeReviewReopen()

/**
 * 将 Thread 重新打开
 * POST /code_review/{thread_id}/reopen
 */
export function useReopenThread(link: string) {
  const queryClient = useQueryClient()

  return useMutation<PostApiCodeReviewReopenData, Error, { threadId: string; params?: RequestParams }>({
    mutationFn: ({ threadId, params }) => reopenThreadMutation.request(threadId, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.invalidateQueries({ queryKey })
    }
  })
}
