import { useMemo, useState } from 'react'

import { getMarkdownExtensions } from '@gitmono/editor'
import { Comment } from '@gitmono/types'

import { AttachmentLightbox } from '@/components/AttachmentLightbox'
import { RichTextRenderer } from '@/components/RichTextRenderer'
import { containsOnlyReactions } from '@/utils/reactions/containsOnlyReactions'

interface CommentRendererProps {
  comment: Comment
  onCheckboxClick?: ({ index, checked }: { index: number; checked: boolean }) => void
}

export function CommentRenderer(props: CommentRendererProps) {
  const { comment, onCheckboxClick } = props
  const hasReactionsOnly = useMemo(() => containsOnlyReactions(comment.body_html), [comment.body_html])
  const [openAttachmentId, setOpenAttachmentId] = useState<string | undefined>()
  // empty link unfurl options are needed to enable it
  const extensions = useMemo(() => getMarkdownExtensions({ linkUnfurl: {} }), [])
  const options = useMemo(() => {
    return {
      taskItem: { onCheckboxClick },
      postNoteAttachment: { onOpenAttachment: setOpenAttachmentId },
      mediaGallery: { onOpenAttachment: setOpenAttachmentId }
    }
  }, [onCheckboxClick])

  return (
    <div className='prose select-text focus:outline-hidden' data-reactions-only={hasReactionsOnly}>
      <AttachmentLightbox
        selectedAttachmentId={openAttachmentId}
        attachments={comment.attachments}
        viewOnly
        onClose={() => setOpenAttachmentId(undefined)}
        onSelectAttachment={({ id }) => setOpenAttachmentId(id)}
      />

      <RichTextRenderer content={comment.body_html} extensions={extensions} options={options} />
    </div>
  )
}
