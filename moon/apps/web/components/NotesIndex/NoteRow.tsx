import { memo } from 'react'
import { format } from 'date-fns'

import { Note } from '@gitmono/types'
import { GlobeIcon, HighlightedCommandItem, LockIcon, NoteFilledIcon, Tooltip, UIText } from '@gitmono/ui'
import { cn } from '@gitmono/ui/src/utils'

import { NoteOverflowMenu } from '@/components/NoteOverflowMenu'
import { useHandleCommandListSubjectSelect } from '@/components/Projects/hooks/useHandleHighlightedItemSelect'
import { ProjectTag } from '@/components/ProjectTag'
import { ViewerFollowUpTag } from '@/components/ViewerFollowUpTag'
import { useScope } from '@/contexts/scope'
import { encodeCommandListSubject } from '@/utils/commandListSubject'

import { NotePrivacyIndicator } from '.'

interface NoteRowProps {
  note: Note
  display?: 'default' | 'pinned' | 'search'
  hideProject?: boolean
  permission?: { hasRead: boolean; hasWrite: boolean; isAdmin: boolean }
}

export const NoteRow = memo(({ note, display = 'default', hideProject = false, permission }: NoteRowProps) => {
  const { scope } = useScope()
  const { handleSelect } = useHandleCommandListSubjectSelect()

  const isAdmin = permission?.isAdmin || false
  const hasRead = permission?.hasRead || false

  const href = `/${scope}/notes/${note.id}`
  const canAccess = isAdmin || hasRead || !!note.viewer_is_author
  const isDisabled = !canAccess

  const handleSelectWithPermission = (value: string) => {
    if (isDisabled) return
    handleSelect(value)
  }

  if (display === 'pinned') {
    const content = (
      <div
        className={cn('relative flex items-center gap-3 px-3 py-2.5', {
          'cursor-not-allowed opacity-50': isDisabled
        })}
      >
        <NoteOverflowMenu type='context' note={note}>
          <HighlightedCommandItem
            className='absolute inset-0 z-0'
            value={encodeCommandListSubject(note, { href, pinned: true })}
            onSelect={handleSelectWithPermission}
            disabled={isDisabled}
          />
        </NoteOverflowMenu>

        <div
          className={cn(
            'flex h-11 w-11 items-center justify-center rounded-full',
            isDisabled
              ? 'bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-600'
              : 'bg-blue-50 text-blue-500 dark:bg-blue-900/50'
          )}
        >
          {isDisabled ? <LockIcon size={24} strokeWidth='2' /> : <NoteFilledIcon size={24} />}
        </div>

        <UIText weight='font-medium' className='break-anywhere line-clamp-1 max-w-lg'>
          {note.title || 'Untitled'}
        </UIText>
      </div>
    )

    return isDisabled ? (
      <Tooltip label='You do not have permission to view this document' side='top' align='start'>
        {content}
      </Tooltip>
    ) : (
      content
    )
  }

  const content = (
    <div
      className={cn('relative flex items-center gap-3 px-3 py-2.5', {
        'cursor-not-allowed opacity-50': isDisabled
      })}
    >
      <NoteOverflowMenu type='context' note={note}>
        <HighlightedCommandItem
          className='absolute inset-0 z-0'
          value={encodeCommandListSubject(note, { href })}
          onSelect={handleSelectWithPermission}
          disabled={isDisabled}
        />
      </NoteOverflowMenu>

      <div className='flex flex-1 items-center gap-3'>
        {isDisabled ? (
          <div className='flex h-5 w-5 items-center justify-center rounded bg-gray-100 text-gray-500 dark:bg-gray-800 dark:text-gray-400'>
            <LockIcon size={14} strokeWidth='2.5' />
          </div>
        ) : (
          <NotePrivacyIndicator note={note} />
        )}

        <ViewerFollowUpTag followUps={note.follow_ups} />

        <UIText weight='font-medium' className='break-anywhere line-clamp-1 max-w-lg'>
          {note.title || 'Untitled'}
        </UIText>

        {display === 'search' && (
          <UIText quaternary className='break-anywhere line-clamp-1 flex-1'>
            {format(note.created_at, 'MMM d, yyyy')}
          </UIText>
        )}
      </div>

      <div className='flex items-center gap-1'>
        {note.public_visibility && (
          <Tooltip label='Published to the web'>
            <span className='relative text-blue-500'>
              <GlobeIcon size={18} strokeWidth='2' />
            </span>
          </Tooltip>
        )}
        {note.project && !hideProject && <ProjectTag project={note.project} />}
      </div>
    </div>
  )

  return isDisabled ? (
    <Tooltip label='You do not have permission to view this document' side='top' align='start'>
      {content}
    </Tooltip>
  ) : (
    content
  )
})
NoteRow.displayName = 'NoteRow'
