import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { ReactionRequest } from '@gitmono/types'
import { SyncCustomReaction } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'
import { getStandardReaction, StandardReaction } from '@/utils/reactions'

interface Props {
  reaction: StandardReaction | SyncCustomReaction
}

export function usePostConversationReactions(commentId: string, id: string, type: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ reaction }: Props) =>
      legacyApiClient.v1.postApiConversationReactions().request(commentId, {
        content: getStandardReaction(reaction)?.native,
        comment_type: type
      } as ReactionRequest),
    onSuccess: () => {
      switch (type) {
        case 'issue':
          queryClient.invalidateQueries({
            queryKey: legacyApiClient.v1.getApiIssueDetail().requestKey(id)
          })
          break
        case 'cl':
          queryClient.invalidateQueries({
            queryKey: legacyApiClient.v1.getApiClDetail().requestKey(id)
          })
          break
        default:
          return
      }
    }
  })
}
