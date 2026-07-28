import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { IssueClosedIcon, IssueOpenedIcon, SkipIcon } from '@primer/octicons-react'
// import { useInfiniteQuery } from '@tanstack/react-query'
import { formatDistance, fromUnixTime } from 'date-fns'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'

import { PostApiIssueListData } from '@gitmono/types/generated'

import { BotBadge } from '@/components/BotBadge'
import {
  IndexTabFilter as IssueIndexTabFilter,
  List as IssueList,
  ItemLabels,
  ItemRightIcons,
  ListBanner,
  ListItem
} from '@/components/ClView/ClList'
import {
  AssigneesDropdown,
  AuthorDropdown,
  LabelsDropdown,
  MilestonesDropdown,
  OrderDropdown,
  ProjectsDropdown,
  TypesDropdown,
  useFilterState
} from '@/components/ClView/filters'
// import { MenuItem } from '@gitmono/ui/Menu'

import { MemberHovercard } from '@/components/InlinePost/MemberHovercard'
import { issueIdAtom } from '@/components/Issues/utils/store'
import { useScope } from '@/contexts/scope'
import { useGetIssueLists } from '@/hooks/issues/useGetIssueLists'
import { useGetLabelList } from '@/hooks/useGetLabelList'
import { useSyncedMembers } from '@/hooks/useSyncedMembers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { atomWithWebStorage } from '@/utils/atomWithWebStorage'

import { Pagination } from './Pagenation'

interface IssuesContentProps {
  setFilterQuery?: (query: string) => void
  shouldClearFilters: boolean
  setShouldClearFilters?: (callback: boolean) => void
}

export type ItemsType = NonNullable<PostApiIssueListData['data']>['items']

export function IssuesContent({ setFilterQuery, shouldClearFilters, setShouldClearFilters }: IssuesContentProps) {
  const { scope } = useScope()
  const router = useRouter()

  const [issueList, setIssueList] = useState<ItemsType>([])
  const [numTotal, setNumTotal] = useState(0)
  const [pageSize, _setPageSize] = useState(10)
  const [page, setPage] = useState(1)
  const [isLoading, setIsLoading] = useState(false)
  const [status, setStatus] = useState('open')

  const [_issueId, setIssueId] = useAtom(issueIdAtom)

  const { mutate: issueLists } = useGetIssueLists()
  const { members } = useSyncedMembers()

  const filterState = useFilterState({ scope: scope as string, type: 'issue' })

  const filterStateRef = useRef(filterState)

  const { labels } = useGetLabelList()

  const orderAtom = useMemo(
    () => atomWithWebStorage(`${scope}:issue-order`, { sort: 'Created On', time: 'Newest' }),
    [scope]
  )
  const [order, setOrder] = useAtom(orderAtom)
  const orderRef = useRef(order)

  useEffect(() => {
    orderRef.current = order
    filterStateRef.current = filterState
  }, [order, filterState])

  const fetchIssueListData = useCallback(() => {
    setIsLoading(true)

    const params = filterStateRef.current.toApiParams()
    const currentOrder = orderRef.current
    const additional: any = {
      status,
      asc: currentOrder.time === 'Oldest',
      sort_by: handleSort(currentOrder.sort),
      ...params
    }

    issueLists(
      {
        data: {
          pagination: { page, per_page: pageSize },
          additional: additional
        }
      },
      {
        onSuccess: (response) => {
          const data = response.data

          setIssueList(data?.items ?? [])
          setNumTotal(data?.total ?? 0)
        },
        onError: apiErrorToast,
        onSettled: () => {
          setIsLoading(false)
        }
      }
    )
  }, [page, pageSize, status, issueLists])

  useEffect(() => {
    fetchIssueListData()
  }, [page, pageSize, status, fetchIssueListData])

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
      fetchIssueListData()
    }
  }, [filterState, labels, page, fetchIssueListData])

  const handleOrderChange = useCallback(
    (sort: string, time: string) => {
      if (order.sort === sort && order.time === time) {
        return
      }

      setOrder({ sort, time })
      if (page !== 1) {
        setPage(1)
      } else {
        setTimeout(() => fetchIssueListData(), 0)
      }
    },
    [page, order, setOrder, fetchIssueListData]
  )

  const clearAllFilters = useCallback(() => {
    filterStateRef.current.clearAllFilters()

    if (router.query.q) {
      const newQuery = { ...router.query }

      delete newQuery.q

      router.push(
        {
          pathname: router.pathname,
          query: newQuery
        },
        undefined,
        { shallow: true }
      )
    }

    if (page !== 1) {
      setPage(1)
    } else {
      setTimeout(() => fetchIssueListData(), 0)
    }
  }, [fetchIssueListData, page, router])

  useEffect(() => {
    if (shouldClearFilters) {
      clearAllFilters()
      setShouldClearFilters?.(false)
    }
  }, [shouldClearFilters, clearAllFilters, setShouldClearFilters])

  useEffect(() => {
    setFilterQuery?.(filterState.toQueryString(labels))
  }, [filterState, labels, setFilterQuery])

  const prevLabelsRef = useRef<string[]>([])
  const urlInitTriggeredRef = useRef(false)

  useEffect(() => {
    const q = router.query.q as string
    const currentLabels = filterState.filters.labels
    const prevLabels = prevLabelsRef.current

    if (
      prevLabels.length === 0 &&
      currentLabels.length > 0 &&
      q &&
      q.match(/^label:/) &&
      labels.length > 0 &&
      !urlInitTriggeredRef.current
    ) {
      urlInitTriggeredRef.current = true
      if (page !== 1) {
        setPage(1)
      } else {
        setTimeout(() => {
          fetchIssueListData()
        }, 0)
      }
    }

    prevLabelsRef.current = currentLabels
  }, [filterState.filters.labels, router.query.q, labels.length, page, fetchIssueListData])

  const getStatusIcon = (status: string) => {
    const normalizedStatus = status.toLowerCase()

    switch (normalizedStatus) {
      case 'open':
        return <IssueOpenedIcon className='text-[#378f50]' />
      case 'closed':
        return <IssueClosedIcon className='text-[#8250df]' />
      default:
        return <SkipIcon className='text-[#59636e]' />
    }
  }

  const getIssueDescription = (item: ItemsType[number]) => {
    const normalizedStatus = item.status.toLowerCase()
    const authorNode = (
      <>
        <MemberHovercard username={item.author}>
          <span className='cursor-pointer hover:text-blue-600 hover:underline'>{item.author}</span>
        </MemberHovercard>
        {item.author_is_bot ? (
          <>
            {' '}
            <BotBadge size='sm' />
          </>
        ) : null}
      </>
    )

    switch (normalizedStatus) {
      case 'open':
        return (
          <>
            {authorNode}
            {' opened '}
            {formatDistance(fromUnixTime(item.open_timestamp), new Date(), { addSuffix: true })}
          </>
        )
      case 'closed':
        return (
          <>
            by {authorNode}
            {' was closed '}
            {formatDistance(fromUnixTime(item.updated_at), new Date(), { addSuffix: true })}
          </>
        )
      default:
        return (
          <>
            {authorNode}
            {' updated '}
            {formatDistance(fromUnixTime(item.updated_at), new Date(), { addSuffix: true })}
          </>
        )
    }
  }

  return (
    <>
      <IssueList
        isLoading={isLoading}
        lists={issueList}
        header={
          <ListBanner
            tabfilter={
              <IssueIndexTabFilter
                part={status}
                openTooltip='Issues that are still open and need attention'
                setPart={(newStatus) => {
                  setStatus(newStatus)
                  setPage(1)
                }}
              />
            }
          >
            {/* Author, Labels, Projects, Milestones, Assignees, Types, Order */}
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
          </ListBanner>
        }
      >
        {(issueList) =>
          issueList.map((i) => (
            <ListItem
              key={i.link}
              title={i.title}
              leftIcon={getStatusIcon(i.status)}
              labels={<ItemLabels item={i} />}
              rightIcon={<ItemRightIcons item={i} />}
              onClick={() => {
                setIssueId(i.id)
                router.push(`/${scope}/issue/${i.link}`)
              }}
            >
              <div className='text-xs text-[#59636e]'>
                <span className='mr-2'>#{i.link}</span>
                {getIssueDescription(i)}
              </div>
            </ListItem>
          ))
        }
      </IssueList>

      <Pagination
        totalNum={numTotal}
        currentPage={page}
        pageSize={pageSize}
        onChange={(page: number) => setPage(page)}
      />
    </>
  )
}
