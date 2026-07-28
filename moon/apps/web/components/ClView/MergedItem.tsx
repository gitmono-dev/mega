import { ConversationItem } from '@gitmono/types/generated'
import { ConditionalWrap, Link } from '@gitmono/ui'

import { ActorAvatar } from '@/components/ActorAvatar'
import { BotBadge } from '@/components/BotBadge'
import { useScope } from '@/contexts/scope'
import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { megaUserHandle } from '@/utils/megaUser'

import { MemberHovercard } from '../InlinePost/MemberHovercard'
import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'

interface MergedItemProps {
  conv: ConversationItem
}

/** Keep queue href same-origin: encode scope so values like `//evil.com` cannot open-redirect. */
function queueHref(scope: unknown): string | null {
  if (typeof scope !== 'string' || scope.length === 0) return null

  const path = `/${encodeURIComponent(scope)}/queue/main`

  if (!path.startsWith('/') || path.startsWith('//')) return null

  return path
}

const MergedItem = ({ conv }: MergedItemProps) => {
  const isBot = !!conv.is_bot
  const { data: member } = useGetOrganizationMember({ username: conv.username, enabled: !isBot })
  const { scope } = useScope()
  const profileUsername = member?.user.username || conv.username
  const displayName = megaUserHandle(member?.user, conv.username) || conv.username
  const href = queueHref(scope)

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
          <span>
            {' '}
            merged via the{' '}
            {href ? (
              <Link href={href} className='text-blue-600 underline hover:text-blue-800'>
                queue
              </Link>
            ) : (
              <span>queue</span>
            )}{' '}
            into main
          </span>
        </div>

        <div className='text-sm text-gray-500 hover:text-gray-700'>
          <HandleTime created_at={conv.created_at} />
        </div>
      </div>
    </>
  )
}

export default MergedItem
