import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap } from '@gitmono/ui'

import { ActorAvatar } from '@/components/ActorAvatar'
import { BotBadge } from '@/components/BotBadge'
import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { megaUserHandle } from '@/utils/megaUser'

import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'

interface ApproveItemProps {
  conv: ConversationItem
}

const ApproveItem = ({ conv }: ApproveItemProps) => {
  const isBot = !!conv.is_bot
  const { data: member } = useGetOrganizationMember({ username: conv.username, enabled: !isBot })
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, conv.username) || conv.username

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
          <span className='text-gray-600'> approved this change list</span>
        </div>
        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default ApproveItem
