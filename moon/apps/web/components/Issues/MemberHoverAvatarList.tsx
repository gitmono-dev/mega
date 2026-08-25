import React from 'react'
import { Avatar, AvatarStack } from '@primer/react'

import { SyncOrganizationMember } from '@gitmono/types/generated'

import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { useMemberMap } from '@/components/Issues/utils/sideEffect'

interface MemberHoverAvatarListProps {
  isLeft?: boolean
  /** Actor keys: campsite_user_id, username, or github_login */
  authors: string[]
}

export const MemberHoverAvatarList = ({ authors, isLeft }: MemberHoverAvatarListProps) => {
  const memberMap = useMemberMap()

  const members = authors
    .map((actor) => memberMap.get(actor) as SyncOrganizationMember | undefined)
    .filter((m): m is SyncOrganizationMember => !!m)

  return (
    <AvatarStack alignRight={!isLeft}>
      {members.map((member) => {
        const src = member.user.avatar_urls?.sm || member.user.avatar_urls?.base || ''

        return (
          <MemberHovercard key={member.user.id} username={member.user.username} side='top' align='end'>
            <div>
              <Avatar src={src} />
            </div>
          </MemberHovercard>
        )
      })}
    </AvatarStack>
  )
}
