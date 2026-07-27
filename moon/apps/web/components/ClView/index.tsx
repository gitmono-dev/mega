'use client'

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon,
  XIcon
} from '@primer/octicons-react'
import { format, formatDistance, fromUnixTime } from 'date-fns'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'

import { PostApiClListData } from '@gitmono/types/generated'
import { SearchIcon, Tooltip } from '@gitmono/ui'
import { cn } from '@gitmono/ui/src/utils'

import {
  List as ClList,
  ListBanner as ClListBanner,
  ListItem as ClListItem,
  IndexTabFilter,
  ItemLabels,
  ItemRightIcons
} from '@/components/ClView/ClList'
import {
  AssigneesDropdown,
  AuthorDropdown,
  LabelsDropdown,
  MilestonesDropdown,
  OrderDropdown,
  ProjectsDropdown,
  ReviewDropdown,
  TypesDropdown,
  useFilterState
} from '@/components/ClView/filters'
import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { IssueBreadcrumbIcon } from '@/components/Titlebar/BreadcrumbPageIcons'
import { BreadcrumbTitlebar } from '@/components/Titlebar/BreadcrumbTitlebar'
import { useScope } from '@/contexts/scope'
import { usePostClList } from '@/hooks/CL/usePostClList'
import { useGetLabelList } from '@/hooks/useGetLabelList'
import { useSyncedMembers } from '@/hooks/useSyncedMembers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { atomWithWebStorage } from '@/utils/atomWithWebStorage'
import { megaUserHandle } from '@/utils/megaUser'

import { IndexPageContainer, IndexPageContent } from '../IndexPages/components'
import { Pagination } from '../Issues/Pagenation'
import { clIdAtom } from '../Issues/utils/store'

type ItemsType = NonNullable<PostApiClListData['data']>['items']

export default function CLView() {
  const { scope } = useScope()
  const router = useRouter()

  const [clList, setClList] = useState<ItemsType>([])
  const [numTotal, setNumTotal] = useState(0)
  const [pageSize] = useState(10)
  const [page, setPage] = useState(1)
  const [isLoading, setIsLoading] = useState(false)
  const [status, setStatus] = useState('open')

  const [_clid, setClid] = useAtom(clIdAtom)

  const { mutate: fetchClList } = usePostClList()
  const { members } = useSyncedMembers()

  const authorHandle = useCallback(
    (author: string) => {
      const member = members.find((m) => m.user.username === author || m.user.github_login === author)

      return megaUserHandle(member?.user, author)
    },
    [members]
  )

  const filterState = useFilterState({ scope: scope as string, type: 'cl' })
  const filterStateRef = useRef(filterState)

  const { labels } = useGetLabelList()

  const orderAtom = useMemo(
    () => atomWithWebStorage(`${scope}:cl-order`, { sort: 'Created On', time: 'Newest' }),
    [scope]
  )
  const [order, setOrder] = useAtom(orderAtom)
  const orderRef = useRef(order)

  useEffect(() => {
    orderRef.current = order
    filterStateRef.current = filterState
  }, [order, filterState])

  const searchQuery = useMemo(() => {
    return filterState.toQueryString(labels)
  }, [filterState, labels])

  const clearAllFilters = () => {
    filterState.clearAllFilters()
    if (page !== 1) {
      setPage(1)
    } else {
      setTimeout(() => fetchClListData(), 0)
    }
  }

  const fetchClListData = useCallback(() => {
    setIsLoading(true)

    const params = filterStateRef.current.toApiParams()
    const currentOrder = orderRef.current
    const additional: any = {
      sort_by: handleSort(currentOrder.sort),
      asc: currentOrder.time === 'Oldest',
      status: status, // 始终传递 status，open 时后端会返回 open + draft
      ...params
    }

    fetchClList(
      {
        data: {
          pagination: { page, per_page: pageSize },
          additional
        }
      },
      {
        onSuccess: (response) => {
          const data = response.data

          setClList((data?.items ?? []) as ItemsType)
          setNumTotal(data?.total ?? 0)
        },
        onError: apiErrorToast,
        onSettled: () => setIsLoading(false)
      }
    )
  }, [page, pageSize, status, fetchClList])

  useEffect(() => {
    fetchClListData()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, pageSize, status])

  const handleSort = (str: string): string => {
    switch (str) {
      case 'Created on':
        return 'created_at'
      case 'Last updated':
        return 'updated_at'

      default:
        return 'created_at'
    }
  }

  const handleFilterClose = useCallback(() => {
    if (!filterState.hasChanged()) {
      return
    }
    const currentFilterString = filterState.toQueryString(labels)

    if (!currentFilterString || currentFilterString.trim() === '') {
      return
    }

    if (page !== 1) {
      setPage(1)
    } else {
      fetchClListData()
    }
  }, [filterState, labels, page, fetchClListData])

  const handleOrderChange = useCallback(
    (sort: string, time: string) => {
      if (order.sort === sort && order.time === time) {
        return
      }

      setOrder({ sort, time })
      if (page !== 1) {
        setPage(1)
      } else {
        setTimeout(() => fetchClListData(), 0)
      }
    },
    [order.sort, order.time, setOrder, page, fetchClListData]
  )

  const getStatusIcon = (status: string) => {
    const normalizedStatus = status.toLowerCase()

    switch (normalizedStatus) {
      case 'open':
        return <GitPullRequestIcon className='text-[#378f50]' />
      case 'draft':
        return <GitPullRequestDraftIcon className='text-[#6e7781]' />
      case 'closed':
        return <GitPullRequestClosedIcon className='text-[#d1242f]' />
      case 'merged':
        return <GitMergeIcon className='text-[#8250df]' />
      default:
        return null
    }
  }

  const getDescription = (item: ItemsType[number]) => {
    const normalizedStatus = item.status.toLowerCase()

    const formatFullTime = (timestamp: number) => {
      const date = fromUnixTime(timestamp)
      const offset = date.getTimezoneOffset() / -60

      return format(date, 'MMM d, yyyy, h:mm a') + ` GMT${offset >= 0 ? '+' : ''}${offset}`
    }

    switch (normalizedStatus) {
      case 'open':
        return (
          <>
            opened{' '}
            <Tooltip label={formatFullTime(item.open_timestamp)}>
              <span className='cursor-default'>
                {formatDistance(fromUnixTime(item.open_timestamp), new Date(), { addSuffix: true })}
              </span>
            </Tooltip>{' '}
            by{' '}
            <MemberHovercard username={item.author}>
              <span className='cursor-pointer hover:text-blue-600 hover:underline'>{authorHandle(item.author)}</span>
            </MemberHovercard>
          </>
        )
      case 'draft':
        return (
          <>
            opened{' '}
            <Tooltip label={formatFullTime(item.open_timestamp)}>
              <span className='cursor-default'>
                {formatDistance(fromUnixTime(item.open_timestamp), new Date(), { addSuffix: true })}
              </span>
            </Tooltip>{' '}
            by{' '}
            <MemberHovercard username={item.author}>
              <span className='cursor-pointer hover:text-blue-600 hover:underline'>{authorHandle(item.author)}</span>
            </MemberHovercard>
          </>
        )
      case 'merged':
        if (item.merge_timestamp !== null) {
          return (
            <>
              by{' '}
              <MemberHovercard username={item.author}>
                <span className='cursor-pointer hover:text-blue-600 hover:underline'>{authorHandle(item.author)}</span>
              </MemberHovercard>
              {' was merged '}
              <Tooltip label={formatFullTime(item.merge_timestamp ?? 0)}>
                <span className='cursor-default'>
                  {formatDistance(fromUnixTime(item.merge_timestamp ?? 0), new Date(), { addSuffix: true })}
                </span>
              </Tooltip>
            </>
          )
        } else {
          return ''
        }
      case 'closed':
        return (
          <>
            by{' '}
            <MemberHovercard username={item.author}>
              <span className='cursor-pointer hover:text-blue-600 hover:underline'>{authorHandle(item.author)}</span>
            </MemberHovercard>
            {' was closed '}
            <Tooltip label={formatFullTime(item.updated_at)}>
              <span className='cursor-default'>
                {formatDistance(fromUnixTime(item.updated_at), new Date(), { addSuffix: true })}
              </span>
            </Tooltip>
          </>
        )
      default:
        return null
    }
  }

  return (
    <>
      <IndexPageContainer>
        <BreadcrumbTitlebar>
          <IssueBreadcrumbIcon />
        </BreadcrumbTitlebar>

        <IndexPageContent
          id='/[org]/cl'
          className={cn('@container', 'max-w-full lg:max-w-5xl xl:max-w-6xl 2xl:max-w-7xl')}
        >
          <div className='border-primary bg-primary group flex min-h-[35px] items-center rounded-md border px-3 shadow-xs transition-all focus-within:border-blue-500 focus-within:shadow-md focus-within:ring-2 focus-within:ring-blue-100 hover:border-blue-400 dark:focus-within:ring-blue-900'>
            <div className='text-quaternary flex items-center'>
              <SearchIcon className='h-4 w-4' />
            </div>

            <input
              type='text'
              value={searchQuery}
              readOnly
              placeholder='Filter change list by author, labels , assignee, or review...'
              className='text-secondary placeholder:text-quaternary w-full flex-1 border-none bg-transparent text-sm ring-0 outline-hidden focus:ring-0 focus:outline-hidden'
            />

            {searchQuery && (
              <button
                onClick={() => {
                  clearAllFilters()
                }}
                className='text-quaternary hover:bg-tertiary hover:text-secondary flex items-center justify-center rounded-md p-1 transition-all'
                title='Clear search'
              >
                <XIcon className='h-4 w-4' />
              </button>
            )}
          </div>

          <ClList
            isLoading={isLoading}
            lists={clList}
            header={
              <ClListBanner
                tabfilter={
                  <IndexTabFilter
                    part={status}
                    setPart={(newStatus) => {
                      setStatus(newStatus)
                      setPage(1)
                    }}
                  />
                }
              >
                {/* Author, Labels, Projects, Milestones, Reviews, Assignees, Types, Order */}
                <AuthorDropdown
                  members={members}
                  value={filterState.filters.author}
                  onChange={filterState.setAuthor}
                  onClose={handleFilterClose}
                />
                <LabelsDropdown
                  labels={labels}
                  value={filterState.filters.labels}
                  onChange={filterState.setLabels}
                  onClose={handleFilterClose}
                />
                <ProjectsDropdown />
                <MilestonesDropdown />
                <ReviewDropdown
                  options={['Approved', 'Changes requested', 'Commented', 'Pending']}
                  value={filterState.filters.review || ''}
                  onChange={filterState.setReview}
                  onClose={handleFilterClose}
                />
                <AssigneesDropdown
                  members={members}
                  value={filterState.filters.assignees}
                  onChange={filterState.setAssignees}
                  onClose={handleFilterClose}
                />
                <TypesDropdown />
                <OrderDropdown
                  sortOptions={['Created On', 'Last updated']}
                  timeOptions={['Newest', 'Oldest']}
                  currentSort={order.sort}
                  currentTime={order.time}
                  onChange={handleOrderChange}
                />
              </ClListBanner>
            }
          >
            {(issueList) => {
              return issueList.map((i) => (
                <ClListItem
                  key={i.id}
                  title={i.title}
                  leftIcon={getStatusIcon(i.status)}
                  labels={<ItemLabels item={i} />}
                  rightIcon={<ItemRightIcons item={i} />}
                  onClick={() => {
                    setClid(i.id)
                    router.push(`/${scope}/cl/${i.link}`)
                  }}
                >
                  <div className='text-xs text-[#59636e]'>
                    <span className='mr-2'>#{i.link}</span>
                    {getDescription(i)}
                    {' • ChangeList'}
                  </div>
                </ClListItem>
              ))
            }}
          </ClList>
          <Pagination
            totalNum={numTotal}
            currentPage={page}
            pageSize={pageSize}
            onChange={(page: number) => setPage(page)}
          />

          {/*</div>*/}
        </IndexPageContent>
      </IndexPageContainer>
    </>
  )
}
