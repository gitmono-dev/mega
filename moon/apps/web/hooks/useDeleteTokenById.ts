import { useMutation } from '@tanstack/react-query'

import { DeleteApiUserTokenByKeyIdData } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useDeleteTokenById() {
  return useMutation<DeleteApiUserTokenByKeyIdData, Error, { keyId: string }>({
    mutationFn: ({ keyId }) => legacyApiClient.v1.deleteApiUserTokenByKeyId().request(keyId)
  })
}
