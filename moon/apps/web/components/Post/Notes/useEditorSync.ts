import { useEffect, useState } from 'react'
import { HocuspocusProvider, HocuspocusProviderWebsocket } from '@hocuspocus/provider'
import { toUint8Array } from 'js-base64'
import * as Y from 'yjs'

import { SYNC_URL } from '@gitmono/config'
import { NOTE_SCHEMA_VERSION } from '@gitmono/editor'
import { ApiError } from '@gitmono/types'

import { useScope } from '@/contexts/scope'
import { useCurrentUserIsLoggedIn } from '@/hooks/useCurrentUserIsLoggedIn'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { apiClient } from '@/utils/queryClient'

export type EditorSyncState = 'connecting' | 'connected' | 'disconnected'
type EditorSyncError = 'error' | 'invalid-schema'

interface Props {
  resourceId: string
  resourceType: 'Post' | 'Note'
  initialState: string | null | undefined
}

function buildSyncUrl(scope: string | string[] | undefined, resourceType: string) {
  const params = new URLSearchParams({
    schemaVersion: String(NOTE_SCHEMA_VERSION),
    organization: String(scope ?? ''),
    type: resourceType
  })

  return `${SYNC_URL}?${params.toString()}`
}

export function useEditorSync({ resourceId, resourceType, initialState }: Props) {
  const { scope } = useScope()
  const isLoggedIn = useCurrentUserIsLoggedIn()

  const [syncError, setSyncError] = useState<EditorSyncError | null>(null)
  const [syncState, setSyncState] = useState<EditorSyncState>('connecting')

  const [provider] = useState(() => {
    let document: Y.Doc | undefined

    if (initialState) {
      const ydoc = new Y.Doc()
      const update = toUint8Array(initialState)

      Y.applyUpdate(ydoc, update)

      document = ydoc
    }

    const websocketProvider = new HocuspocusProviderWebsocket({
      url: buildSyncUrl(scope, resourceType),
      autoConnect: false
    })

    return new HocuspocusProvider({
      document,
      websocketProvider,
      name: resourceId,
      token: () =>
        apiClient.users
          .postMeSyncToken()
          .request()
          .then((res) => res.token)
          .catch((error: ApiError) => {
            apiErrorToast(error)
            return ''
          }),
      onStateless: (data) => {
        const message = JSON.parse(data.payload)

        if (message.type === 'schema' && NOTE_SCHEMA_VERSION < message.version) {
          setSyncError('invalid-schema')
        }
      },
      onAuthenticationFailed() {
        setSyncError('error')
      },
      onAuthenticated() {
        // don't clear invalid-schema errors on auth
        if (syncError !== 'invalid-schema') {
          setSyncError(null)
        }
      },
      onStatus(data) {
        setSyncState(data.status)
      }
    })
  })

  useEffect(() => {
    if (isLoggedIn) {
      void provider.connect()
    } else {
      provider.disconnect()
    }

    return () => {
      provider.disconnect()
    }
  }, [provider, isLoggedIn])

  return [provider, syncState, syncError] as const
}
