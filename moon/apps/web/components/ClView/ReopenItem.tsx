import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap } from '@gitmono/ui'

import { ActorAvatar } from '@/components/ActorAvatar'
import { BotBadge } from '@/components/BotBadge'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle, systemEventPhrase } from '@/utils/megaUser'

import { MemberHovercard } from '../InlinePost/MemberHovercard'
import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'

export interface ReopenItemProps {
  conv: ConversationItem
}
const ReopenItem = ({ conv }: ReopenItemProps) => {
  const isBot = !!conv.is_bot
  const { data: member } = useMemberByActor(conv.username, !isBot)
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, conv.username) || conv.username
  const phrase = systemEventPhrase(conv.comment, conv.username, 'reopen this')

  return (
    <>
      <div className='flex items-center space-x-2'>
        <div className='cursor-pointer'>
          <ConditionalWrap
            condition={!isBot}
            wrap={(c) => (
              <MemberHovercard username={profileUsername}>
                <UserLinkByName username={profileUsername} className='relative'>
                  {c}
                </UserLinkByName>
              </MemberHovercard>
            )}
          >
            <ActorAvatar member={member} username={conv.username} isBot={isBot} size='sm' />
          </ConditionalWrap>
        </div>
        <div className='flex flex-wrap items-center gap-1.5'>
          <span className='font-semibold'>{displayName}</span>
          {isBot && <BotBadge size='sm' />}
          <span>{phrase}</span>
        </div>
        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default ReopenItem
