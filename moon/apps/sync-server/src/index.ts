import { Logger } from '@hocuspocus/extension-logger'
import { Server } from '@hocuspocus/server'

import { PORT } from './config'
import { database, getResource, sendVersionToConnections } from './database'
import { AuthenticationError, Context } from './types'

const server = new Server({
  port: PORT,

  async onAuthenticate(data): Promise<Context> {
    if (!data.token) {
      throw new AuthenticationError('no-token')
    }

    const schemaVersion = parseInt(data.requestParameters.get('schemaVersion') || '', 10)
    const organization = data.requestParameters.get('organization')
    const type = data.requestParameters.get('type')

    if (!organization) {
      throw new AuthenticationError('invalid-type')
    }

    try {
      const state = await getResource({ token: data.token, id: data.documentName, type, organization })

      if (!state) {
        throw new AuthenticationError('invalid-type')
      }

      const document = data.instance.documents.get(data.documentName)

      if (document) sendVersionToConnections(document, state.description_schema_version)
      data.connectionConfig.readOnly = schemaVersion < state.description_schema_version

      return {
        token: data.token,
        schemaVersion,
        organization,
        type
      }
    } catch (error) {
      console.error('onAuthenticate failed', {
        document: { id: data.documentName, organization, type },
        schemaVersion,
        error
      })
      throw error
    }
  },

  extensions: [database, new Logger()]
})

server.listen()
