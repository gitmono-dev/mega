import { useState } from 'react'
import { useAtomValue } from 'jotai'
import { useRouter } from 'next/router'

import { MessageThread } from '@gitmono/types'
import { Button, cn, Link, UIText, UnreadSquareBadgeIcon } from '@gitmono/ui'
import { HoverCard } from '@gitmono/ui/src/HoverCard'

import { sidebarCollapsedAtom } from '@/components/Layout/AppLayout'
import { MemberStatus } from '@/components/MemberStatus'
import { ChatFavoriteButton } from '@/components/Thread/ChatFavoriteButton'
import { useScope } from '@/contexts/scope'
import { useMarkThreadUnread } from '@/hooks/useMarkThreadUnread'

import { ThreadView } from './ThreadView'

interface Props extends React.PropsWithChildren {
  thread: MessageThread
  onOpenChange: (newVal: boolean) => void
  disabled?: boolean
}

export function ThreadHoverCard({ children, thread, onOpenChange, disabled }: Props) {
  const router = useRouter()
  const { scope } = useScope()
  const [open, setOpen] = useState(false)
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom)
  const isViewingThread = thread.id === router.query.threadId
  const isDisabled = sidebarCollapsed || isViewingThread || disabled
  const href = `/${scope}/chat/${thread.id}`
  const { mutate: markThreadUnread } = useMarkThreadUnread()
  const handleOpenChange = (newVal: boolean) => {
    setOpen(newVal)
    onOpenChange(newVal)
  }

  return (
    <HoverCard open={open} onOpenChange={handleOpenChange} disabled={isDisabled} targetHref={href}>
      <HoverCard.Trigger asChild>{children}</HoverCard.Trigger>
      <HoverCard.Content sideOffset={4} alignOffset={-44} portalContainer='quick-thread'>
        <HoverCard.Content.TitleBar>
          <div className='flex flex-1 items-center gap-1'>
            <Link href={href} onClick={() => handleOpenChange(false)} className='flex items-center gap-1 p-1'>
              <UIText weight='font-semibold' className='break-anywhere line-clamp-1'>
                {thread?.title}
              </UIText>
              {thread?.other_members.length === 1 && <MemberStatus status={thread.other_members.at(0)?.status} />}
            </Link>

            <ChatFavoriteButton thread={thread} />
          </div>

          <Button
            tooltip='Mark unread'
            variant='plain'
            iconOnly={<UnreadSquareBadgeIcon />}
            accessibilityLabel='Mark unread'
            onClick={() => markThreadUnread({ threadId: thread.id })}
          />
        </HoverCard.Content.TitleBar>

        <div
          className={cn(
            'flex flex-1 flex-col overflow-hidden',
            // inherit hovercard content border radius, the hovercard itself can't
            // be styled with `overflow-hidden` as it will break the prediction cone
            'rounded-b-lg'
          )}
        >
          <ThreadView threadId={thread.id} placement='hovercard' />
        </div>
      </HoverCard.Content>
    </HoverCard>
  )
}
