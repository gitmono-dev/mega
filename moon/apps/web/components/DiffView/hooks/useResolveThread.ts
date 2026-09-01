import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { PostApiCodeReviewResolveData, RequestParams } from '@gitmono/types/generated'

import { legacyApiClient } from '@/utils/queryClient'

const resolveThreadMutation = legacyApiClient.v1.postApiCodeReviewResolve()

/**
 * 将 Thread 标记为已解决
 * POST /code_review/{thread_id}/resolve
 */
export function useResolveThread(link: string) {
  const queryClient = useQueryClient()

  return useMutation<PostApiCodeReviewResolveData, Error, { threadId: string; params?: RequestParams }>({
    mutationFn: ({ threadId, params }) => resolveThreadMutation.request(threadId, params),

    onSuccess: () => {
      const queryKey = legacyApiClient.v1.getApiCodeReviewComments().requestKey(link)

      queryClient.invalidateQueries({ queryKey })
    }
  })
}
