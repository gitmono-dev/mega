import { PropsWithChildren } from 'react'
import Router from 'next/router'

import { NoteFilledIcon, PostFilledIcon, UIText } from '@gitmono/ui'
import { HighlightedCommandItem } from '@gitmono/ui/Command'

import { useScope } from '@/contexts/scope'
import { useGetNote } from '@/hooks/useGetNote'
import { useGetPost } from '@/hooks/useGetPost'

import { LocalNote, LocalPost, useSyncRecentlyViewedItem } from './utils'

type RecentlyViewedContainerProps = PropsWithChildren & {
  value: string
  onSelect: () => void
  route: string
}

function RecentlyViewedContainer({ onSelect, value, children, route }: RecentlyViewedContainerProps) {
  return (
    <HighlightedCommandItem
      value={value}
      className='group h-9 gap-2 pr-1.5'
      onSelect={() => {
        onSelect()
        Router.push(route)
      }}
    >
      {children}
    </HighlightedCommandItem>
  )
}

export function RecentlyViewedPost({ post, onSelect }: { post: LocalPost; onSelect: () => void }) {
  const { data: syncedPost, isError } = useGetPost({ postId: post.id, enabled: false })
  const { scope } = useScope()

  useSyncRecentlyViewedItem({ id: post.id, post: syncedPost, isError })

  return (
    <RecentlyViewedContainer value={post.id} route={`/${scope}/posts/${post.id}`} onSelect={onSelect}>
      <PostFilledIcon className='text-quaternary flex-none' />
      <UIText className='line-clamp-1' weight='font-medium'>
        {post.title || 'Untitled post'}
      </UIText>
    </RecentlyViewedContainer>
  )
}

export function RecentlyViewedNote({ note, onSelect }: { note: LocalNote; onSelect: () => void }) {
  const { data: syncedNote, isError } = useGetNote({ id: note.id, enabled: false })
  const { scope } = useScope()

  useSyncRecentlyViewedItem({ id: note.id, note: syncedNote, isError })

  return (
    <RecentlyViewedContainer value={note.id} onSelect={onSelect} route={`/${scope}/notes/${note.id}`}>
      <NoteFilledIcon className='flex-none text-blue-500' />
      <UIText className='line-clamp-1' weight='font-medium'>
        {note.title || 'Untitled doc'}
      </UIText>
    </RecentlyViewedContainer>
  )
}
