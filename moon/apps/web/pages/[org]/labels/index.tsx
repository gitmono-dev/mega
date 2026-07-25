import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { XIcon } from '@primer/octicons-react'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'
import { useDebounce } from 'use-debounce'

import { LabelItem } from '@gitmono/types'
import { Button, cn, LazyLoadingSpinner, SearchIcon } from '@gitmono/ui'

import { Heading } from '@/components/ClView/catalyst/heading'
// import { IssueList as LabelList, ListItem } from '@/components/Issues/IssueList';
import { List as LabelList, ListItem } from '@/components/ClView/ClList'
import { IndexPageContainer, IndexPageContent } from '@/components/IndexPages/components'
import { Pagination } from '@/components/Issues/Pagenation'
import { NewLabelDialog } from '@/components/Labels/NewLabelDialog'
import { AppLayout } from '@/components/Layout/AppLayout'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { BreadcrumbTitlebar, BreadcrumbTitlebarContainer } from '@/components/Titlebar/BreadcrumbTitlebar'
import { useScope } from '@/contexts/scope'
import { usePostLabelList } from '@/hooks/usePostLabelList'
import { usePostLabelNew } from '@/hooks/usePostLabelNew'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { atomWithWebStorage } from '@/utils/atomWithWebStorage'
import { getFontColor } from '@/utils/getFontColor'

function LabelsPage() {
  const { scope } = useScope()
  const router = useRouter()

  const [query, setQuery] = useState('')
  const [queryDebounced] = useDebounce(query, 150)

  const [isLoading, setIsLoading] = useState(false)
  const [isSearchLoading, setIsSearchLoading] = useState(false)

  const labelsAtom = useMemo(() => atomWithWebStorage<LabelItem[]>(`${scope}:label`, []), [scope])
  const [labelList, setLabelList] = useAtom(labelsAtom)

  const [numTotal, setNumTotal] = useState(0)

  const [pageSize] = useState(20)
  const [page, setPage] = useState(1)

  const [isNewLabelDialogOpen, setIsNewLabelDialogOpen] = useState(false)

  const { mutate: fetchLabelList } = usePostLabelList()
  const { mutate: postNewLabel } = usePostLabelNew()

  const fetchLabels = useCallback(() => {
    setIsLoading(true)
    setIsSearchLoading(true)

    fetchLabelList(
      {
        data: {
          additional: queryDebounced,
          pagination: {
            page,
            per_page: pageSize
          }
        }
      },
      {
        onSuccess: (response) => {
          const data = response.data

          setLabelList(data?.items ?? [])
          setNumTotal(data?.total ?? 0)
        },
        onError: apiErrorToast,
        onSettled: () => {
          setIsLoading(false)
          setIsSearchLoading(false)
        }
      }
    )
  }, [fetchLabelList, queryDebounced, page, pageSize, setLabelList])

  useEffect(() => {
    fetchLabels()
  }, [fetchLabels])

  const handleCreateLabel = (name: string, description: string, color: string) => {
    postNewLabel(
      {
        data: {
          name,
          description,
          color
        }
      },
      {
        onSuccess: () =>
          fetchLabelList(
            {
              data: {
                additional: '',
                pagination: {
                  page: 1,
                  per_page: pageSize
                }
              }
            },
            {
              onSuccess: (response) => {
                const data = response.data

                setLabelList(data?.items ?? [])
                setNumTotal(data?.total ?? 0)
                setQuery('')
              },
              onError: apiErrorToast
            }
          ),
        onError: apiErrorToast
      }
    )

    setIsNewLabelDialogOpen(false)
  }

  const clearQuery = () => {
    setQuery('')
    if (page !== 1) {
      setPage(1)
    } else {
      setTimeout(() => fetchLabels(), 0)
    }
  }

  return (
    <>
      <BreadcrumbTitlebar>
        <Heading>Labels</Heading>
      </BreadcrumbTitlebar>

      <IndexPageContainer>
        <IndexPageContent
          id='/[org]/labels'
          className={cn('@container', 'max-w-full lg:max-w-3xl xl:max-w-4xl 2xl:max-w-5xl')}
        >
          <div className='flex min-h-[35px] items-center gap-2'>
            <div className='relative flex flex-1 flex-row items-center gap-2 overflow-hidden rounded-md border border-gray-300 px-2 focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500'>
              <input
                className='flex-1 border-none bg-transparent p-0 text-sm ring-0 outline-hidden focus:ring-0'
                placeholder='Search...'
                role='searchbox'
                autoComplete='off'
                autoCorrect='off'
                spellCheck={false}
                type='text'
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value)
                }}
              />

              {query && (
                <button
                  onClick={() => {
                    clearQuery()
                  }}
                  className='flex items-center justify-center rounded-md p-1 pr-4 text-gray-400 transition-all hover:bg-gray-100 hover:text-gray-600'
                  title='Clear search'
                >
                  <XIcon className='h-4 w-4' />
                </button>
              )}

              <span className='text-tertiary flex w-5 items-center justify-center'>
                <div className='border-l !border-l-[#d1d9e0]'>
                  <Button variant='plain' className='rounded-none bg-[#f6f8fa]' tooltip='search'>
                    {isSearchLoading ? <LazyLoadingSpinner fallback={<SearchIcon />} /> : <SearchIcon />}
                  </Button>
                </div>
              </span>
            </div>

            <Button
              variant='primary'
              className='bg-[#1f883d]'
              size={'base'}
              onClick={() => setIsNewLabelDialogOpen(true)}
            >
              New label
            </Button>
          </div>

          <LabelList
            isLoading={isLoading}
            lists={labelList}
            header={
              <BreadcrumbTitlebarContainer className='justify-between bg-gray-100 pr-3 pl-3'>
                <span className='p-2 font-medium'>{numTotal} labels</span>
              </BreadcrumbTitlebarContainer>
            }
          >
            {(labels) => {
              return labels.map((label) => {
                const fontColor = getFontColor(label.color)

                return (
                  <ListItem
                    key={label.id}
                    title={''}
                    onClick={() => router.push(`/${scope}/issue?q=label:${label.name}`)}
                    rightIcon={<div className='self-auto text-center text-sm text-gray-500'>{label.description}</div>}
                  >
                    <div
                      className='w-full justify-center rounded-[16px] px-2 py-1 text-center text-xs font-semibold'
                      style={{
                        backgroundColor: label.color,
                        color: fontColor.toHex()
                      }}
                    >
                      {label.name}
                    </div>
                  </ListItem>
                )
              })
            }}
          </LabelList>

          <Pagination
            totalNum={numTotal}
            currentPage={page}
            pageSize={pageSize}
            onChange={(page: number) => setPage(page)}
          />
        </IndexPageContent>
      </IndexPageContainer>

      <NewLabelDialog
        isOpen={isNewLabelDialogOpen}
        onClose={() => setIsNewLabelDialogOpen(false)}
        onCreateLabel={handleCreateLabel}
      />
    </>
  )
}

LabelsPage.getProviders = (
  page: React.ReactNode,
  pageProps: React.JSX.IntrinsicAttributes & { children?: React.ReactNode }
) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default LabelsPage
