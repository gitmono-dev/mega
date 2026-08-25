import { useState } from 'react'
import { Flex } from '@radix-ui/themes'
import { formatDistanceToNow } from 'date-fns'
import router from 'next/router'

import { Avatar, Button, ClockIcon, EyeIcon } from '@gitmono/ui'

import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { useGetLatestCommit } from '@/hooks/useGetLatestCommit'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle } from '@/utils/megaUser'

const CommitHyStyle = {
  width: '100%',
  borderRadius: 8
}

interface CommitHistoryProps {
  flag: string
  path?: string
  refs?: string
}

export default function CommitHistory({ flag, path, refs }: CommitHistoryProps) {
  const [Expand, setExpand] = useState(false)
  const { data: commitData } = useGetLatestCommit(path, refs)
  const { data: member } = useMemberByActor(commitData?.author)
  const displayName = megaUserHandle(member?.user, commitData?.author || '')
  const hoverUsername = member?.user.username || commitData?.author || ''

  const ExpandDetails = () => {
    setExpand(!Expand)
  }

  if (!commitData) {
    return null
  }

  const commit = commitData
  // Convert Unix timestamp (in seconds) to milliseconds for Date object
  const dateInMs = commit.date ? parseInt(commit.date) * 1000 : 0
  const formattedDate = dateInMs ? formatDistanceToNow(new Date(dateInMs), { addSuffix: true }) : ''
  const shortHash = commit.oid?.substring(0, 7) || ''

  return (
    <>
      <div style={CommitHyStyle} className='border-primary bg-primary border'>
        <Flex align='center' className='min-h-[50px] p-1'>
          <MemberHovercard username={hoverUsername} role='member'>
            <Flex align='center'>
              <Avatar src={member?.user?.avatar_urls?.sm || member?.user?.avatar_urls?.base || ''} />
              <span className='text-primary mx-3 font-bold'>{displayName || commit.author}</span>
            </Flex>
          </MemberHovercard>
          <span className='text-tertiary min-w-0 flex-1 truncate text-sm'>{commit.short_message}</span>
          {flag === 'contents' && (
            <Flex>
              <Button
                size='sm'
                variant='plain'
                className='ml-1 p-0'
                tooltip='Open commit details'
                onClick={ExpandDetails}
              >
                <EyeIcon size={24} />
              </Button>
            </Flex>
          )}

          <span className='text-quaternary mr-3 ml-auto text-xs'>
            {shortHash} · {formattedDate}
          </span>

          {!(path === '/third-party') && (
            <Button
              size='large'
              variant='plain'
              className='flex items-center'
              onClick={() => {
                const { org } = router.query

                router.push(`/${org}/code/commits/${refs || 'main'}${path ? `/${path}` : ''}`)
              }}
            >
              <Flex align='center'>
                <ClockIcon size={24} />
                <span className='ml-2'>History</span>
              </Flex>
            </Button>
          )}
        </Flex>
      </div>

      {Expand && commitData && (
        <p className='text-tertiary ml-4'>
          Signed-off-by: {displayName || commitData.author} {'<'}
          {member?.user?.email}
          {'>'}
        </p>
      )}
    </>
  )
}
