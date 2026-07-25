import { useEffect, useMemo, useState } from 'react'
import { formatDistanceToNow } from 'date-fns'
import { useRouter } from 'next/router'

import { TagResponse } from '@gitmono/types'
import { Avatar, Button, DotsHorizontal, Link, LinkIcon, TrashIcon, UIText } from '@gitmono/ui'
import { DropdownMenu } from '@gitmono/ui/DropdownMenu'
import { buildMenuItems } from '@gitmono/ui/Menu'
import { useCopyToClipboard } from '@gitmono/ui/src/hooks'

import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { useDeleteMonoTag } from '@/hooks/useDeleteMonoTag'

interface Props {
  tags: TagResponse[]
  defaultPath?: string
  onDelete?: (name: string) => void
}

export function MonoTagList({ tags, defaultPath, onDelete }: Props) {
  const [localTags, setLocalTags] = useState(tags)

  useEffect(() => {
    setLocalTags(tags)
  }, [tags])
  if (!localTags?.length) return null
  return (
    <ul className='flex flex-col py-2'>
      {localTags.map((t) => (
        <MonoTagRow
          key={t.name}
          tag={t}
          defaultPath={defaultPath}
          onDelete={() => {
            setLocalTags(localTags.filter((tag) => tag.name !== t.name))
            if (typeof onDelete === 'function') onDelete(t.name)
          }}
        />
      ))}
    </ul>
  )
}

function MonoTagRow({ tag, defaultPath, onDelete }: { tag: TagResponse; defaultPath?: string; onDelete?: () => void }) {
  return <InnerRow tag={tag} defaultPath={defaultPath} onDelete={onDelete} />
}

function InnerRow({ tag, defaultPath, onDelete }: { tag: TagResponse; defaultPath?: string; onDelete?: () => void }) {
  const [copy] = useCopyToClipboard()
  const [menuOpen, setMenuOpen] = useState(false)
  const del = useDeleteMonoTag()
  const router = useRouter()

  const org = router.query.org || 'mega'

  const subtitle = useMemo(() => {
    return tag.message || `${tag.object_type} ${tag.object_id.substring(0, 8)}`
  }, [tag])

  const creator = useMemo(() => {
    // tagger format examples: "Alice <alice@example.com>", "alice@example.com", "unknown"
    const t = (tag.tagger || '').trim()

    if (!t) return { name: 'unknown', email: undefined as string | undefined }
    const m = t.match(/^(.*)\s*<([^>]+)>$/)

    if (m) {
      const name = m[1].trim() || m[2].split('@')[0]

      return { name, email: m[2] }
    }
    if (t.includes('@')) {
      return { name: t.split('@')[0], email: t }
    }
    return { name: t, email: undefined as string | undefined }
  }, [tag.tagger])

  const username = useMemo(() => {
    if (creator.email) return creator.email.split('@')[0]
    return creator.name || ''
  }, [creator])

  const createdRelative = useMemo(() => {
    try {
      return formatDistanceToNow(new Date(tag.created_at), { addSuffix: true })
    } catch {
      return ''
    }
  }, [tag.created_at])

  const href = useMemo(() => {
    const path = (defaultPath || '').replace(/^\/+/, '')

    return `/${org}/code/tree/${tag.name}/${path}`
  }, [org, tag.name, defaultPath])

  return (
    <li className='hover:bg-tertiary group-has-[button[aria-expanded="true"]]:bg-tertiary group relative -mx-3 flex items-center gap-3 rounded-md py-1.5 pr-1.5 pl-3'>
      <Link href={href} className='absolute inset-0 z-0' />

      {/* removed leading hashtag icon per design update */}

      <div className='flex flex-1 flex-col gap-0.5'>
        <UIText weight='font-medium' size='text-[15px]' className='line-clamp-1'>
          {tag.name}
        </UIText>
        {subtitle && (
          <UIText quaternary size='text-[12px]' className='line-clamp-1'>
            {subtitle}
          </UIText>
        )}
        {tag.tagger && tag.tagger !== 'unknown' ? (
          <MemberHovercard username={username} role='member'>
            <div className='relative z-[1] mt-0.5 flex items-center gap-1.5'>
              <Avatar size='xs' name={creator.name} />
              <UIText quaternary size='text-[12px]' className='line-clamp-1'>
                Created by {creator.name} · {createdRelative ? `${createdRelative}` : ''}
              </UIText>
            </div>
          </MemberHovercard>
        ) : (
          <UIText quaternary size='text-[12px]' className='mt-0.5 line-clamp-1'>
            Created {createdRelative ? `${createdRelative}` : ''}
          </UIText>
        )}
      </div>

      <div className='hidden flex-none items-center gap-0.5 lg:flex'>
        <div className='flex opacity-0 group-hover:opacity-100 group-has-[button[aria-expanded="true"]]:opacity-100'>
          <DropdownMenu
            trigger={<Button variant='plain' iconOnly={<DotsHorizontal />} accessibilityLabel='Open menu' />}
            open={menuOpen}
            onOpenChange={setMenuOpen}
            align='end'
            items={buildMenuItems([
              {
                type: 'item',
                leftSlot: <LinkIcon />,
                label: 'Copy name',
                onSelect: () => {
                  void copy(tag.name)
                }
              },
              {
                type: 'item',
                leftSlot: <TrashIcon isAnimated />,
                label: 'Delete',
                destructive: true,
                onSelect: () => {
                  del.mutate(tag.name, {
                    onSuccess: () => {
                      if (onDelete) onDelete()
                    }
                  })
                }
              }
            ])}
          />
        </div>
      </div>
    </li>
  )
}
