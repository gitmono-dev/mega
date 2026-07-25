import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap } from '@gitmono/ui'

import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'

import { MemberHovercard } from '../InlinePost/MemberHovercard'
import { MemberAvatar } from '../MemberAvatar'
import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'

interface CloseItemProps {
  conv: ConversationItem
}
const CloseItem = ({ conv }: CloseItemProps) => {
  const { data: member } = useGetOrganizationMember({ username: conv.username })
  const profileUsername = member?.user.username || conv.username

  return (
    <>
      <div className='flex items-center space-x-2'>
        <div className='cursor-pointer'>
          <ConditionalWrap
            condition={true}
            wrap={(c) => (
              <MemberHovercard username={profileUsername}>
                <UserLinkByName username={profileUsername} className='relative'>
                  {c}
                </UserLinkByName>
              </MemberHovercard>
            )}
          >
            {member ? <MemberAvatar member={member} size='sm' /> : 'Avatar not found'}
          </ConditionalWrap>
        </div>
        <div>{conv.comment}</div>
        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default CloseItem
