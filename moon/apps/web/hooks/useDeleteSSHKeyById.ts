import { useMutation } from '@tanstack/react-query'

import { DeleteApiUserSshByKeyIdData } from '@gitmono/types'

import { legacyApiClient } from '@/utils/queryClient'

export function useDeleteSSHKeyById() {
  return useMutation<DeleteApiUserSshByKeyIdData, Error, { keyId: string }>({
    mutationFn: ({ keyId }) => legacyApiClient.v1.deleteApiUserSshByKeyId().request(keyId)
  })
}
