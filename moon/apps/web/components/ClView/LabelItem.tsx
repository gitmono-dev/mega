import { useMemo } from 'react'
import { useQueries } from '@tanstack/react-query'

import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap } from '@gitmono/ui'

import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { getFontColor } from '@/utils/getFontColor'
import { megaUserHandle } from '@/utils/megaUser'
import { legacyApiClient } from '@/utils/queryClient'

import { MemberHovercard } from '../InlinePost/MemberHovercard'
import { MemberAvatar } from '../MemberAvatar'
import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'

interface LabelItemProps {
  conv: ConversationItem
}

function LabelItem({ conv }: LabelItemProps) {
  const { data: member } = useGetOrganizationMember({ username: conv.username })
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, conv.username)
  const comment = conv.comment?.split(' ') ?? []

  const idList = useMemo(() => {
    const match = conv.comment?.match(/\[(.*?)]/)

    if (!match || match.length <= 1) return []
    return match[1]
      .split(', ')
      .map((id) => parseInt(id, 10))
      .filter((id) => !isNaN(id))
  }, [conv.comment])

  const labelQueries = useQueries({
    queries: idList.map((id) => ({
      queryKey: ['label', id],
      queryFn: () => legacyApiClient.v1.getApiLabelById().request(id),
      enabled: id > 0
    }))
  })

  const labels = labelQueries.filter((q) => q.data?.data).map((q) => q.data!.data!)

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
          {labels.map((label) => {
            const fontColor = getFontColor(label.color)

            return (
              <span
                key={label.id}
                style={{
                  backgroundColor: label.color,
                  color: fontColor.toHex(),
                  borderRadius: '16px',
                  padding: '0px 8px',
                  fontSize: '12px',
                  fontWeight: '550',
                  justifyContent: 'center',
                  textAlign: 'center'
                }}
                className='mr-1'
              >
                {label.name}
              </span>
            )
          })}
        </div>
        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default LabelItem
