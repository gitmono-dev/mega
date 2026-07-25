import { Database } from '@hocuspocus/extension-database'
import { Document } from '@hocuspocus/server'
import { TiptapTransformer } from '@hocuspocus/transformer'
import { generateHTML, generateJSON } from '@tiptap/html'
import { fromUint8Array, toUint8Array } from 'js-base64'
import * as Y from 'yjs'

import { getNoteExtensions } from '@gitmono/editor'

import { api } from './api'
import { Context } from './types'

const extensions = getNoteExtensions()

export function sendVersionToConnections(document: Document, version: number) {
  document.getConnections().forEach((connection) => {
    const connectionSchemaVersion = (connection.context as Context | undefined)?.schemaVersion ?? 0

    // Update connections to readOnly if the schema version is lower than the current version
    connection.readOnly = connectionSchemaVersion < version

    // Send the schema version to the client
    connection.sendStateless(
      JSON.stringify({
        type: 'schema',
        version
      })
    )
  })
}

interface GetResourceProps {
  token: string
  id: string
  type: string | null
  organization: string
}

export async function getResource({ token, id, type, organization }: GetResourceProps) {
  if (type === 'Note') {
    return api.organizations.getNotesSyncState().request(organization, id, {
      headers: { Authorization: `Bearer ${token}` }
    })
  }
}

function resolveContext(data: {
  context?: Context
  lastContext?: Context
  requestParameters?: URLSearchParams
}): Context | undefined {
  const fromPayload = data.lastContext ?? data.context

  if (fromPayload?.token && fromPayload.organization) {
    return fromPayload
  }

  const organization = data.requestParameters?.get('organization') ?? fromPayload?.organization
  const type = data.requestParameters?.get('type') ?? fromPayload?.type ?? null
  const token = fromPayload?.token
  const schemaVersion = fromPayload?.schemaVersion ?? 0

  if (!token || !organization) return fromPayload

  return { token, schemaVersion, organization, type }
}

export const database = new Database({
  /**
   * Fetch the document state from Campsite, or generate a new document from the existing
   * HTML if the document has never been edited before.
   */
  async fetch(data) {
    const context = resolveContext(data)

    const id = data.documentName
    const organization = context?.organization
    const type = context?.type ?? null

    try {
      if (!context?.token || !organization) return new Uint8Array()

      const state = await getResource({ token: context.token, id, type, organization })

      if (!state) {
        return new Uint8Array()
      }

      sendVersionToConnections(data.document, state.description_schema_version)

      // If there's a state (a.k.a, it has been edited before), return it
      if (state.description_state) {
        return toUint8Array(state.description_state)
      }

      // Otherwise, generate a new state from the HTML
      const json = generateJSON(state.description_html, extensions)
      const ydoc = TiptapTransformer.toYdoc(json, 'default', extensions)

      return Y.encodeStateAsUpdate(ydoc)
    } catch (error) {
      console.error('database.fetch failed', {
        document: { id, organization, type },
        schemaVersion: context?.schemaVersion,
        error
      })
      throw error
    }
  },

  /**
   * Store the document state in Campsite.
   */
  async store(data) {
    const context = resolveContext(data)

    const id = data.documentName
    const organization = context?.organization
    const type = context?.type ?? null

    try {
      if (!context?.token || !organization) return

      // Generate a state from the Yjs document
      const state = Y.encodeStateAsUpdate(data.document)
      const dbDocument = fromUint8Array(state)

      // Generate HTML from the Yjs document
      const json = TiptapTransformer.fromYdoc(data.document, 'default')
      const html = generateHTML(json, extensions)

      // Push the state (for Y.js) and the HTML (for our API) to Campsite
      await api.organizations.putNotesSyncState().request(
        organization,
        id,
        {
          description_html: html,
          description_state: dbDocument,
          description_schema_version: context.schemaVersion
        },
        {
          headers: { Authorization: `Bearer ${context.token}` }
        }
      )
    } catch (error) {
      console.error('database.store failed', {
        document: { id, organization, type },
        schemaVersion: context?.schemaVersion,
        error
      })
      throw error
    }
  }
})
