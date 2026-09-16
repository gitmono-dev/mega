import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap } from '@gitmono/ui'

import { ActorAvatar } from '@/components/ActorAvatar'
import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle } from '@/utils/megaUser'

import HandleTime from './components/HandleTime'
import { MegaUserLabel } from './components/MegaUserLabel'
import { UserLinkByName } from './components/UserLinkByName'

interface ReviewerEventItemProps {
  conv: ConversationItem
}

type ReviewerEvent =
  | { kind: 'assigned'; actor: string; reviewer: string }
  | { kind: 'removed'; actor: string; reviewer: string }
  | { kind: 'resolved'; actor: string }

export function parseReviewerEventComment(comment?: string | null): ReviewerEvent | null {
  if (!comment) return null
  const text = comment.trim()

  let match = text.match(/^(\S+)\s+assigned a new reviewer\s+(\S+)$/)

  if (match) {
    return { kind: 'assigned', actor: match[1], reviewer: match[2] }
  }

  match = text.match(/^(\S+)\s+removed reviewer\s+(\S+)$/)

  if (match) {
    return { kind: 'removed', actor: match[1], reviewer: match[2] }
  }

  match = text.match(/^(\S+)\s+resolved a review$/)

  if (match) {
    return { kind: 'resolved', actor: match[1] }
  }

  return null
}

const ReviewerEventItem = ({ conv }: ReviewerEventItemProps) => {
  const event = parseReviewerEventComment(conv.comment)
  const actorKey = event?.actor || conv.username
  const { data: member } = useMemberByActor(conv.username)
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, actorKey) || actorKey

  if (!event) return null

  const phrase =
    event.kind === 'assigned'
      ? 'assigned a new reviewer'
      : event.kind === 'removed'
        ? 'removed reviewer'
        : 'resolved a review'

  return (
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
          <ActorAvatar member={member} username={conv.username} size='sm' />
        </ConditionalWrap>
      </div>
      <div className='flex flex-wrap items-center gap-1.5'>
        <span className='font-semibold'>{displayName}</span>
        <span className='text-gray-600'>{phrase}</span>
        {(event.kind === 'assigned' || event.kind === 'removed') && (
          <span className='cursor-pointer font-semibold text-[#1f2328] underline'>
            <MegaUserLabel username={event.reviewer} />
          </span>
        )}
      </div>
      <div className='text-sm text-gray-500 hover:text-gray-700'>
        <HandleTime created_at={conv.created_at} />
      </div>
    </div>
  )
}

export default ReviewerEventItem
