import React from 'react'

import { LabelItem, SyncOrganizationMember } from '@gitmono/types'
import { cn } from '@gitmono/ui/src/utils'

import { MemberAvatar } from '@/components/MemberAvatar'

export const DropdownItemwithAvatar = ({
  member,
  classname
}: {
  member: SyncOrganizationMember
  classname?: string
}) => {
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md border-l-4 border-transparent p-2 hover:border-[#0969da]',
        classname
      )}
    >
      <MemberAvatar size='sm' member={member} />
      <span className='text-primary text-sm font-semibold'>{member.user.display_name}</span>
      <span className='text-tertiary ml-1 text-xs'>{member.user.github_login?.trim() || member.user.username}</span>
    </div>
  )
}

export const DropdownItemwithLabel = ({ classname, label }: { classname?: string; label: LabelItem }) => {
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md border-l-4 border-transparent p-2 hover:border-[#0969da]',
        classname
      )}
    >
      <div
        className='h-3.5 w-3.5 rounded-full border'
        style={{ backgroundColor: label.color, borderColor: label.color }}
      />
      <span className='text-primary text-sm font-semibold'>{label.name}</span>
      <span className='text-tertiary ml-1 text-xs'>{label.description}</span>
    </div>
  )
}
