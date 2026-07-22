import { useMutation } from '@tanstack/react-query'

import type { GenerateCedarRequest } from '@gitmono/types'

import { apiErrorToast } from '@/utils/apiErrorToast'
import { legacyApiClient } from '@/utils/queryClient'

export function useGenerateMegaCedar() {
  const api = legacyApiClient.v1.postApiAdminCedarGenerate()

  return useMutation({
    mutationFn: (data: GenerateCedarRequest) => api.request(data),
    onError: apiErrorToast
  })
}
