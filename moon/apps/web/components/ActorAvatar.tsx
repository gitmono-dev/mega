import { ComponentProps } from 'react'

import { Avatar } from '@gitmono/ui/Avatar'

import { MemberAvatar } from '@/components/MemberAvatar'

type AvatarSize = NonNullable<ComponentProps<typeof Avatar>['size']>

type MemberLike = ComponentProps<typeof MemberAvatar>['member']

/**
 * Member avatar when available; otherwise a bot/initials avatar so timeline
 * rows never show "Avatar not found" for mega-init and other bots.
 */
export function ActorAvatar({
  member,
  username,
  isBot,
  size = 'sm'
}: {
  member?: MemberLike | null
  username: string
  isBot?: boolean
  size?: AvatarSize
}) {
  if (member) {
    return <MemberAvatar member={member} size={size} />
  }

  return (
    <Avatar
      name={username || (isBot ? 'bot' : '?')}
      size={size}
      rounded={isBot ? 'rounded' : 'rounded-full'}
      tooltip={username}
    />
  )
}
