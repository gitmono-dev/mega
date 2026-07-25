import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ChainedCommands, Editor } from '@tiptap/core'
import { v4 as uuid } from 'uuid'

import { useScope } from '@/contexts/scope'
import { useCreateAttachment } from '@/hooks/useCreateAttachment'
import { createOptimisticAttachment } from '@/utils/createFileUploadPipeline'

import { setOptimisticAttachment } from '../Post/Notes/Attachments/useUploadAttachments'

/**
 * Creates link attachments, which are URL-based inline attachments.
 */
export function useCreateLinkAttachment() {
  const { scope } = useScope()
  const queryClient = useQueryClient()
  const { mutateAsync: createAttachment } = useCreateAttachment()

  const createLink = useCallback(
    async ({ url, editor, chain }: { url: string; editor: Editor; chain: () => ChainedCommands }) => {
      const clientId = uuid()

      const tempAttachment = createOptimisticAttachment({
        id: clientId,
        optimistic_id: clientId,
        optimistic_file_path: 'link',
        link: true,
        image: false,
        file_type: 'link'
      })

      setOptimisticAttachment({ queryClient, scope, value: tempAttachment })

      chain().insertAttachments([tempAttachment])

      const attachment = await createAttachment({
        ...tempAttachment,
        file_path: url
      })

      setOptimisticAttachment({
        queryClient,
        scope,
        value: {
          ...attachment,
          optimistic_id: tempAttachment.id
        }
      })

      editor.commands.updateAttachment(tempAttachment.id, attachment)
    },
    [queryClient, scope, createAttachment]
  )

  return createLink
}
