import { useAtomValue } from 'jotai'
import { useRouter } from 'next/router'

import { ChatBubbleIcon, HomeIcon, NoteIcon } from '@gitmono/ui/Icons'

import { ChatHoverList } from '@/components/Chat/ChatHoverCard'
import { sidebarCollapsedAtom } from '@/components/Layout/AppLayout'
import { useRefetchNotesIndex, useRefetchPostsIndex } from '@/components/NavigationBar/useNavigationTabAction'
import { NotesHoverList } from '@/components/NotesIndex/NotesHoverCard'
import { SidebarLink, SidebarProps } from '@/components/Sidebar/SidebarLink'
import { SidebarUnreadBadge } from '@/components/Sidebar/SidebarUnreadBadge'
import { useScope } from '@/contexts/scope'
import { useCurrentUserOrOrganizationHasFeature } from '@/hooks/useCurrentUserOrOrganizationHasFeature'
import { useGetUnreadNotificationsCount } from '@/hooks/useGetUnreadNotificationsCount'
import { useMarkIndexPageRead } from '@/hooks/useMarkIndexPageUnread'

export function SidebarMessages({ label, href, active }: SidebarProps) {
  const { scope } = useScope()
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom)
  const hasSidebarDms = useCurrentUserOrOrganizationHasFeature('sidebar_dms')
  const getUnreadNotificationsCount = useGetUnreadNotificationsCount()
  const unreadDMCount = getUnreadNotificationsCount.data?.messages[`${scope}`] || 0
  const hasUnreadDMs = unreadDMCount > 0

  if (!hasSidebarDms) {
    return null
  }

  return (
    <ChatHoverList alignOffset={-44} sideOffset={4} disabled={sidebarCollapsed}>
      <SidebarLink
        id='chat'
        label={label}
        href={href}
        active={active}
        unread={hasUnreadDMs}
        trailingAccessory={hasUnreadDMs && <SidebarUnreadBadge important={false}>{unreadDMCount}</SidebarUnreadBadge>}
        leadingAccessory={<ChatBubbleIcon />}
      />
    </ChatHoverList>
  )
}

export function SidebarDocs({ label, href, active }: SidebarProps) {
  const router = useRouter()
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom)
  const refetchNotes = useRefetchNotesIndex()
  const isViewingNotes = router.pathname === '/[org]/notes'

  function onNotesClick() {
    refetchNotes()
  }

  return (
    <NotesHoverList alignOffset={-44} sideOffset={4} disabled={sidebarCollapsed || isViewingNotes}>
      <SidebarLink
        id='notes'
        label={label}
        href={href}
        active={active}
        leadingAccessory={<NoteIcon />}
        onClick={onNotesClick}
      />
    </NotesHoverList>
  )
}

export function SidebarHome({ label, href, active }: SidebarProps) {
  const refetchPosts = useRefetchPostsIndex()

  const { mutate: markIndexPageRead } = useMarkIndexPageRead()

  function onPostsClick() {
    refetchPosts()
    markIndexPageRead()
  }

  return (
    <SidebarLink
      id='posts'
      label={label}
      href={href}
      active={active}
      leadingAccessory={<HomeIcon />}
      onClick={onPostsClick}
    />
  )
}
