import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { CurrentUser } from '@gitmono/types'

import { apiClient, setTypedQueriesData } from '@/utils/queryClient'

import { useBindCurrentUserEvent } from './useBindCurrentUserEvent'

const getMe = apiClient.users.getMe()
const getFavorites = apiClient.organizations.getFavorites()

export const useCurrentUserSubscriptions = () => {
  const queryClient = useQueryClient()

  const updateCurrentUser = useCallback(
    ({ current_user }: { current_user: CurrentUser }) => {
      setTypedQueriesData(queryClient, getMe.requestKey(), current_user)
    },
    [queryClient]
  )

  const invalidateFavorites = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: getFavorites.baseKey })
  }, [queryClient])

  const invalidateAccessTokens = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: apiClient.integrations.getIntegrationsCalDotComIntegration().baseKey })
  }, [queryClient])

  useBindCurrentUserEvent('current-user-stale', updateCurrentUser)
  useBindCurrentUserEvent('favorites-stale', invalidateFavorites)
  useBindCurrentUserEvent('access-tokens-stale', invalidateAccessTokens)
}
