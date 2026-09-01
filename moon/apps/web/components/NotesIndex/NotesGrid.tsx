import { useState } from 'react'
import { useTheme } from 'next-themes'
import Image from 'next/image'

import { Note } from '@gitmono/types/generated'
import { Link } from '@gitmono/ui/Link'
import { UIText } from '@gitmono/ui/Text'
import { Tooltip } from '@gitmono/ui/Tooltip'
import { cn } from '@gitmono/ui/utils'

import { useScope } from '@/contexts/scope'

import { NoteOwnerAvatar } from '.'
import { NoteOverflowMenu } from '../NoteOverflowMenu'
import { NoteFavoriteButton } from './NoteFavoriteButton'

function NotePreviewThumbnail({ note }: { note: Note }) {
  const { resolvedTheme } = useTheme()
  const baseUrl = note.description_thumbnail_base_url
  const [showPlaceholder, setShowPlaceholder] = useState(true)
  const urlsize = 700
  const url = `${baseUrl}/${urlsize}/${resolvedTheme}`

  return (
    <div className='dark:bg-elevated dark:hover:bg-tertiary relative h-full w-full overflow-hidden rounded-lg border border-transparent p-4 ring-1 ring-black/5 transition-all dark:border-black dark:shadow-[inset_0_0_0_1px_rgba(255,255,255,0.02),_inset_0_1px_0_rgba(255,255,255,0.04)]'>
      {!showPlaceholder && (
        <UIText
          size='text-[9px]'
          weight='font-bold'
          className='break-anywhere mb-1 line-clamp-3 leading-snug tracking-tighter'
        >
          {note.title || 'Untitled'}
        </UIText>
      )}

      {showPlaceholder && (
        <div className='absolute inset-0 flex h-full w-full flex-col gap-2.5 p-4'>
          <div className='bg-tertiary h-2 w-[88%] rounded' />
          <div className='bg-tertiary h-2 w-[80%] rounded' />
          <div className='bg-tertiary h-2 w-[90%] rounded' />
          <div className='bg-tertiary h-2 w-[84%] rounded' />
          <div className='bg-tertiary h-2 w-[70%] rounded' />
        </div>
      )}

      <Image
        alt={`Text post preview`}
        src={url}
        draggable={false}
        className={cn('relative w-full max-w-full object-contain transition-opacity', {
          'opacity-0': showPlaceholder
        })}
        width={urlsize / 2}
        height={urlsize / 2}
        onLoad={() => setShowPlaceholder(false)}
        onLoadStart={() => setShowPlaceholder(true)}
      />

      <div className='dark:from-gray-850 absolute right-px bottom-0 left-px z-[1] h-full rounded-b-lg bg-gradient-to-t from-white via-transparent to-transparent' />
    </div>
  )
}

function NoteGridItem({
  note,
  hideProject = false,
  permission
}: {
  note: Note
  hideProject?: boolean
  permission?: { hasRead: boolean; hasWrite: boolean; isAdmin: boolean }
}) {
  const { scope } = useScope()

  const isAdmin = permission?.isAdmin || false
  const hasRead = permission?.hasRead || false

  const canAccess = isAdmin || hasRead || !!note.viewer_is_author
  const isDisabled = !canAccess

  const gridContent = (
    <div
      className={cn('group flex aspect-[3/4] w-full flex-col gap-3', {
        'cursor-not-allowed opacity-40': isDisabled
      })}
    >
      <div className='relative h-full w-full'>
        {isDisabled ? (
          <div className='cursor-not-allowed'>
            <NotePreviewThumbnail note={note} />
          </div>
        ) : (
          <Link href={`/${scope}/notes/${note.id}`}>
            <NotePreviewThumbnail note={note} />
          </Link>
        )}
        <div
          className={cn(
            'absolute right-2 bottom-2 z-[2] flex -space-x-2 group-hover:opacity-100 group-data-[state="open"]:opacity-100',
            {
              'opacity-100': note.viewer_has_favorited,
              'opacity-0': !note.viewer_has_favorited
            }
          )}
        >
          <NoteFavoriteButton note={note} />
        </div>
        <div className='absolute right-3 bottom-3 z-[2] flex -space-x-2'>
          <NoteOwnerAvatar
            note={note}
            className='text-quaternary dark:bg-gray-750 z-[2] flex h-7 w-7 items-center justify-center rounded-full border border-transparent bg-white shadow ring-[0.5px] ring-black/10 dark:border-gray-900 dark:shadow-[inset_0_0.5px_0_rgba(255,255,255,0.1)]'
          />
        </div>
        {isDisabled && (
          <div className='absolute top-2 right-2 z-[3]'>
            <span className='flex h-6 w-6 items-center justify-center rounded-full bg-gray-900/80 text-xs text-white'>
              🔒
            </span>
          </div>
        )}
      </div>

      <div className='flex flex-col gap-px px-2'>
        <div className='flex items-center gap-3'>
          {isDisabled ? (
            <div className='flex-1 cursor-not-allowed'>
              <UIText weight='font-medium' className='break-anywhere line-clamp-2'>
                {note.title || 'Untitled'}
              </UIText>
            </div>
          ) : (
            <Link href={`/${scope}/notes/${note.id}`} className='flex-1'>
              <UIText weight='font-medium' className='break-anywhere line-clamp-2'>
                {note.title || 'Untitled'}
              </UIText>
            </Link>
          )}

          <div
            className={cn(
              'flex h-5 flex-none self-start opacity-50 group-hover:opacity-100 group-has-[button[aria-expanded="true"]]:opacity-100 group-data-[state="open"]:opacity-100 lg:opacity-0 lg:group-hover:opacity-100 lg:group-data-[state="open"]:opacity-100'
            )}
          >
            <NoteOverflowMenu type='dropdown' note={note} />
          </div>
        </div>
        {note.project && !hideProject && (
          <Link
            href={`/${scope}/projects/${note.project.id}`}
            className='text-quaternary hover:text-primary flex items-center gap-1.5 self-start'
          >
            {note.project.accessory && (
              <UIText inherit className='font-["emoji"] leading-none' size='text-xs'>
                {note.project.accessory}
              </UIText>
            )}
            <UIText inherit className='break-anywhere line-clamp-1'>
              {note.project.name}
            </UIText>
          </Link>
        )}
      </div>
    </div>
  )

  return (
    <NoteOverflowMenu key={note.id} type='context' note={note}>
      {isDisabled ? (
        <Tooltip label='You do not have permission to view this document'>{gridContent}</Tooltip>
      ) : (
        gridContent
      )}
    </NoteOverflowMenu>
  )
}

export function NotesGrid({
  notes,
  hideProject,
  notesPermissions
}: {
  notes: Note[]
  hideProject?: boolean
  notesPermissions?: Record<string, { hasRead: boolean; hasWrite: boolean; isAdmin: boolean }>
}) {
  return (
    <div className='grid grid-cols-2 gap-x-4 gap-y-8 @xl:grid-cols-3 @2xl:grid-cols-4 @2xl:gap-x-6 @4xl:grid-cols-5'>
      {notes.map((note) => (
        <NoteGridItem note={note} key={note.id} hideProject={hideProject} permission={notesPermissions?.[note.id]} />
      ))}
    </div>
  )
}
