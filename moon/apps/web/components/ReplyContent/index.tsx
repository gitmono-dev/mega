import pluralize from 'pluralize'

import { Attachment, User } from '@gitmono/types'
import { Button, CloseIcon, UIText } from '@gitmono/ui'

import { EMPTY_HTML } from '@/atoms/markdown'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'

export function ReplyContent({
  content,
  author,
  onCancel,
  attachments
}: {
  content: string
  author: User
  onCancel(): void
  attachments: Attachment[]
}) {
  const { data: currentUser } = useGetCurrentUser()

  const replyHasContent = !!content.length && content !== EMPTY_HTML
  const replyToIsViewer = author.id === currentUser?.id
  const replyTo = replyToIsViewer ? 'your message' : author.display_name

  return (
    <div className='bg-tertiary flex items-center gap-2 rounded-[18px] pt-2 pr-2 pb-2 pl-4'>
      <div className='flex flex-1 flex-col'>
        <div className='text-secondary flex items-center gap-1'>
          <UIText inherit size='text-[13px]' className='line-clamp-1'>
            Replying to {replyTo}
          </UIText>
        </div>
        <div className='text-tertiary chat-prose line-clamp-1 text-sm'>
          {replyHasContent ? (
            <div dangerouslySetInnerHTML={{ __html: content }} />
          ) : (
            <p>
              {attachments.length} {pluralize('attachment', attachments.length)}
            </p>
          )}
        </div>
      </div>
      <Button
        type='button'
        round
        onClick={onCancel}
        iconOnly={<CloseIcon strokeWidth='2.5' size={16} />}
        variant='plain'
        accessibilityLabel='Remove reply'
        className='text-tertiary hover:text-primary hover:bg-black/5 dark:hover:bg-white/10'
      />
    </div>
  )
}
