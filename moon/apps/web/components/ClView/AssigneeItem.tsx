import { ConditionalWrap } from '@gitmono/ui/utils'

import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { megaUserHandle } from '@/utils/megaUser'

import { MemberHovercard } from '../InlinePost/MemberHovercard'
import { MemberAvatar } from '../MemberAvatar'
import HandleTime from './components/HandleTime'
import { MegaUserLabel } from './components/MegaUserLabel'
import { UserLinkByName } from './components/UserLinkByName'
import { ReopenItemProps } from './ReopenItem'

const AssigneeItem = ({ conv }: ReopenItemProps) => {
  const match = conv.comment?.match(/\["(.*?)"\]/) ?? ''
  const comment = conv.comment?.split(' ') ?? []
  const { data: member } = useGetOrganizationMember({ username: conv.username })
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, conv.username)

  const assignees = match[1].split('", "')

  return (
    <>
      <div className='flex flex-wrap items-center gap-2'>
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
        <div>
          <span className='font-semibold'>{displayName} </span>
          <span className='text-gray-400'>{comment[1]} </span>
          {assignees &&
            assignees.map((i, index) => (
              <ConditionalWrap
                key={i}
                condition={true}
                wrap={(c) => (
                  <MemberHovercard username={i}>
                    <UserLinkByName username={i} className='relative'>
                      {c}
                    </UserLinkByName>
                  </MemberHovercard>
                )}
              >
                <>
                  <span className='cursor-pointer text-[#1f2328] underline'>
                    <MegaUserLabel username={i} />
                    {index < assignees.length - 1 && ', '}
                  </span>
                </>
              </ConditionalWrap>
            ))}
        </div>
        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default AssigneeItem
