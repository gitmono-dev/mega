import { useRouter } from 'next/router'

import { Favorite } from '@gitmono/types/generated'
import { NoteFilledIcon, PostFilledIcon } from '@gitmono/ui/Icons'

import { useDeleteFavorite } from '@/hooks/useDeleteFavorite'

import { SidebarChatThread } from './SidebarChatThread'
import { SidebarLink } from './SidebarLink'
import { SidebarProject } from './SidebarProject'

interface Props {
  favorite: Favorite
  isDragging: boolean
  onPeek: (id?: string) => void
}

export function iconForFavoritableType(favoritableType: Favorite['favoritable_type'], size = 20) {
  switch (favoritableType) {
    case 'Note':
      return <NoteFilledIcon size={size} className='text-blue-500' />
    case 'Post':
      return <PostFilledIcon size={size} />
    default:
      return null
  }
}

export function fallbackNameForFavoritableType(favoritableType: Favorite['favoritable_type']) {
  switch (favoritableType) {
    case 'Note':
      return 'Untitled doc'
    default:
      return undefined
  }
}

export function SidebarFavorite({ favorite, isDragging, onPeek }: Props) {
  const deleteFavorite = useDeleteFavorite()
  const router = useRouter()
  const isInbox = router.pathname.startsWith('/[org]/inbox/[inboxView]')

  function onRemove() {
    deleteFavorite.mutate(favorite)
  }

  if (favorite.project) {
    return (
      <SidebarProject
        project={favorite.project}
        onRemove={onRemove}
        removeTooltip='Remove favorite'
        isDragging={isDragging}
        location='favorites'
      />
    )
  }

  if (favorite.message_thread) {
    return (
      <SidebarChatThread
        thread={favorite.message_thread}
        onRemove={onRemove}
        removeTooltip='Remove favorite'
        onPeek={onPeek}
        isDragging={isDragging}
        location='favorites'
      />
    )
  }

  return (
    <SidebarLink
      id={favorite.id}
      label={favorite.name ?? fallbackNameForFavoritableType(favorite.favoritable_type)}
      href={favorite.url}
      active={!isInbox && favorite.url.endsWith(router.asPath)}
      leadingAccessory={iconForFavoritableType(favorite.favoritable_type)}
      onRemove={onRemove}
    />
  )
}
