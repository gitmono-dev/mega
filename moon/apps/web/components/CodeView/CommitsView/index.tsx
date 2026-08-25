import React, { useEffect, useMemo, useState } from 'react'
import { ChevronUpDownIcon } from '@heroicons/react/24/solid'
import { CopyIcon } from '@primer/octicons-react'
import copy from 'copy-to-clipboard'
import { format, formatDistance, fromUnixTime } from 'date-fns'
import { useSetAtom } from 'jotai'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { GpgStatus } from '@gitmono/types'
import { GitCommitIcon } from '@gitmono/ui'

import { ListBanner } from '@/components/ClView/ClList'
import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { MemberHoverAvatarList } from '@/components/Issues/MemberHoverAvatarList'
import { Pagination } from '@/components/Issues/Pagenation'
import { useMemberMap } from '@/components/Issues/utils/sideEffect'
import { useScope } from '@/contexts/scope'
import { useGetCommitsHistory } from '@/hooks/commits/useGetCommitsHistory'
import { megaUserHandle } from '@/utils/megaUser'

import { AuthorDropdown, BranchDropdown, DateRangeValue, TimeDropdown } from './dropdown'
import { commitPath, CommitsItem, CommitsList } from './items'
import { mockMembers } from './mock'

const formatCommitDate = (timestamp: number): string => {
  const dateObject = fromUnixTime(timestamp)

  return format(dateObject, 'MMM d, yyyy')
}

/** Resolve campsite_user_id / username / github_login to a display label. */
export function FormatAssignees({ assignees }: { assignees: string[] }) {
  const memberMap = useMemberMap()
  const resolve = (actor: string) => {
    const member = memberMap.get(actor)

    return {
      display: megaUserHandle(member?.user, actor),
      hoverUsername: member?.user.username || actor
    }
  }

  if (assignees.length === 1) {
    const a = resolve(assignees[0])

    return (
      <MemberHovercard username={a.hoverUsername}>
        <span className='text-accent cursor-pointer transition-colors hover:underline'>{a.display}</span>
      </MemberHovercard>
    )
  }

  if (assignees.length === 2) {
    const a = resolve(assignees[0])
    const b = resolve(assignees[1])

    return (
      <>
        <MemberHovercard username={a.hoverUsername}>
          <span className='text-accent cursor-pointer transition-colors hover:underline'>{a.display}</span>
        </MemberHovercard>
        {' and '}
        <MemberHovercard username={b.hoverUsername}>
          <span className='text-accent cursor-pointer transition-colors hover:underline'>{b.display}</span>
        </MemberHovercard>
      </>
    )
  }

  if (assignees.length >= 3) {
    return <>{`${assignees.length} people`}</>
  }

  return null
}

type Commits = {
  author: string
  committer: string
  date: string
  gpg_status?: GpgStatus
  parents: string[]
  sha: string
  short_message: string
}[]

export const CommitsView: React.FC = () => {
  const { scope } = useScope()

  const members = mockMembers
  const setCommitPath = useSetAtom(commitPath)
  const { mutate: commitslist, isPending: isLoadingCommits } = useGetCommitsHistory()
  const [commitsList, setCommitsList] = useState<Commits>([])
  const [numTotal, setNumTotal] = useState(0)

  // Filter states
  const [branchState, setBranchState] = useState<string>('main')
  const [authorState, setAuthorState] = useState<string>('')
  const [timeState, setTimeState] = useState<DateRangeValue>({ from: undefined, to: undefined })

  const [pageNum, setPageNum] = useState<number>(1)
  const [pageSize] = useState(35)

  const router = useRouter()
  const { path } = router.query
  const fullPath = Array.isArray(path) ? path.join('/') : path || ''

  // Handle filter close callback
  const handleFilterClose = (_value: unknown) => {
    // Can be used for analytics or additional logic when filter closes
  }

  const groupedCommits = useMemo((): [string, Commits][] => {
    const grouped = new Map<string, Commits>()

    for (const commit of commitsList) {
      const dateObject = fromUnixTime(parseInt(commit.date, 10))
      const dateKey = format(dateObject, 'MMM d, yyyy')

      if (!grouped.has(dateKey)) {
        grouped.set(dateKey, [])
      }
      grouped.get(dateKey)?.push(commit)
    }

    return Array.from(grouped.entries())
  }, [commitsList])

  useEffect(() => {
    commitslist(
      {
        data: {
          additional: {
            path: fullPath,
            author: authorState,
            refs: branchState
          },
          pagination: {
            page: pageNum,
            per_page: pageSize
          }
        }
      },
      {
        onSuccess: (response) => {
          const data = response.data

          setCommitPath(fullPath)
          setCommitsList((data?.items ?? []) as Commits)
          setNumTotal(data?.total ?? 0)
        }
      }
    )
  }, [authorState, branchState, commitslist, fullPath, pageNum, pageSize, setCommitPath])

  return (
    <>
      <CommitsList
        isLoading={isLoadingCommits}
        lists={groupedCommits}
        header={
          <ListBanner
            tabfilter={<BranchDropdown value={branchState} onChange={setBranchState} onClose={handleFilterClose} />}
          >
            <div className='flex gap-2'>
              <AuthorDropdown
                members={members}
                value={authorState}
                onChange={setAuthorState}
                onClose={handleFilterClose}
              />
              <TimeDropdown members={members} value={timeState} onChange={setTimeState} onClose={handleFilterClose} />
            </div>
          </ListBanner>
        }
      >
        {(groupedCommits: [string, Commits][]) => {
          return groupedCommits.map(([dateKey, dailyCommits]) => (
            <div key={dateKey}>
              <div className='relative'>
                {dailyCommits.map((item, index) => {
                  const commitDate = formatCommitDate(Number(item.date))

                  return (
                    <div key={item.sha} className='relative flex gap-4'>
                      <div className='relative flex w-5 flex-col items-center'>
                        <div className='bg-border-muted absolute top-0 bottom-0 left-1/2 w-0.5 -translate-x-1/2 transform' />
                        {index === 0 && (
                          <div className='bg-primary relative z-10 mt-3 shrink-0 rounded-full'>
                            <GitCommitIcon size={18} className='text-tertiary' />
                          </div>
                        )}
                      </div>

                      <div className='flex flex-1 flex-col'>
                        <div className='text-secondary py-3 text-sm'>Commits on {commitDate}</div>

                        <div className='flex-1'>
                          <CommitsItem
                            key={item.sha}
                            title={item.short_message}
                            labels={
                              <span
                                className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium ${
                                  item.gpg_status === 'Verified'
                                    ? 'border-green-600 text-green-600 dark:border-green-500 dark:text-green-500'
                                    : item.gpg_status === 'Unverified'
                                      ? 'border-yellow-600 text-yellow-600 dark:border-yellow-500 dark:text-yellow-500'
                                      : 'border-border-default text-tertiary'
                                }`}
                              >
                                {item.gpg_status}
                              </span>
                            }
                            sha={
                              <span className='text-tertiary items-center font-mono text-xs'>
                                {item.sha.substring(0, 7)}
                              </span>
                            }
                            copyIcon={
                              <button
                                onClick={(e) => {
                                  e.stopPropagation()
                                  void copy(item.sha).then((ok) =>
                                    ok ? toast.success('Copied to clipboard') : toast.error('Copy failed')
                                  )
                                }}
                                className='text-tertiary hover:text-secondary transition-colors'
                              >
                                <CopyIcon className='h-4 w-4' />
                              </button>
                            }
                            rightIcon={<ChevronUpDownIcon className='text-tertiary h-5 w-5 rotate-90' />}
                            onClick={() => {
                              router.push(`/${scope}/code/commit/${item.sha}`)
                            }}
                          >
                            <div className='text-tertiary flex items-center gap-2 text-xs leading-4'>
                              <div className='h-5 shrink-0'>
                                <MemberHoverAvatarList authors={[item.committer]} isLeft={true} />
                              </div>
                              <div className='flex items-center gap-1 whitespace-nowrap'>
                                <FormatAssignees assignees={[item.committer]} />
                                <span className='text-quaternary text-[11px]'>
                                  authored{' '}
                                  {formatDistance(fromUnixTime(parseInt(item.date, 10)), new Date(), {
                                    addSuffix: true
                                  })}
                                </span>
                              </div>
                            </div>
                          </CommitsItem>
                        </div>
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>
          ))
        }}
      </CommitsList>
      <Pagination
        totalNum={numTotal}
        currentPage={pageNum}
        pageSize={pageSize}
        onChange={(page: number) => setPageNum(page)}
      />
    </>
  )
}

export default CommitsView
