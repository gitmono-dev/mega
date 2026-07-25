import React, { useMemo, useRef, useState } from 'react'
import { useRouter } from 'next/router'

import {
  Button,
  CloseIcon,
  cn,
  CONTAINER_STYLES,
  LazyLoadingSpinner,
  Popover,
  PopoverContent,
  PopoverElementAnchor,
  PopoverPortal,
  SelectCommandContainer,
  SelectCommandEmpty,
  SelectCommandGroup,
  SelectCommandInput,
  SelectCommandItem,
  SelectCommandList,
  SelectCommandSeparator,
  UIText
} from '@gitmono/ui'

import { usePostMonoTagList } from '@/hooks/usePostMonoTagList'

export default function TagSwitcher() {
  const router = useRouter()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const anchorRef = useRef<HTMLButtonElement | null>(null)

  const org = router.query.org as string
  const refs = router.query.version as string

  const pathArray = router.query.path
    ? Array.isArray(router.query.path)
      ? router.query.path
      : [router.query.path]
    : []
  const path = pathArray.join('/')

  const { data, isLoading, isFetching } = usePostMonoTagList({
    additional: '/',
    pagination: { page: 1, per_page: 200 }
  })

  const tags = useMemo(() => data?.data?.items ?? [], [data])

  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase()

    if (!term) return tags
    return tags.filter((t) => t.name.toLowerCase().includes(term))
  }, [query, tags])

  return (
    <>
      {refs && refs !== 'main' && (
        <span ref={anchorRef as unknown as React.RefObject<HTMLSpanElement>}>
          <Button onClick={() => setOpen(true)}>{`Tag: ${refs}`}</Button>
        </span>
      )}

      <Popover open={open} onOpenChange={setOpen} modal>
        <PopoverElementAnchor element={anchorRef.current} />
        <PopoverPortal>
          <PopoverContent
            className={cn('scrollable max-w-[360px] min-w-[360px] p-0', CONTAINER_STYLES.base)}
            side='bottom'
            align='end'
            asChild
          >
            <div className='relative flex max-h-[400px] flex-col'>
              {/* Top-right close button for consistency */}
              <Button
                className='absolute top-2 right-2 z-10'
                variant='plain'
                iconOnly={<CloseIcon strokeWidth='2' />}
                accessibilityLabel='Close'
                tooltip='Close'
                tooltipShortcut='Esc'
                onClick={() => setOpen(false)}
              />
              <SelectCommandContainer className='flex max-h-[400px] flex-col'>
                <div className='flex items-center gap-2 p-2'>
                  <SelectCommandInput placeholder='Find a tag...' value={query} onValueChange={(v) => setQuery(v)} />
                </div>
                <SelectCommandSeparator alwaysRender />

                <SelectCommandList>
                  {isLoading || isFetching ? (
                    <div className='flex items-center justify-center py-6'>
                      <LazyLoadingSpinner />
                    </div>
                  ) : (
                    <>
                      <SelectCommandEmpty>No tags</SelectCommandEmpty>
                      <SelectCommandGroup className='py-1'>
                        {filtered.map((t) => (
                          <SelectCommandItem
                            key={t.name}
                            value={t.name}
                            title={t.name}
                            className='h-auto min-h-8 py-1.5'
                            onSelect={() => {
                              setOpen(false)
                              if (t.name) {
                                router.push(`/${org}/code/tree/${encodeURIComponent(t.name)}/${path}`)
                              }
                            }}
                          >
                            <div className='flex min-w-0 flex-col gap-0.5'>
                              <span className='truncate'>{t.name}</span>
                              {t.message && (
                                <UIText quaternary size='text-[12px]' className='truncate'>
                                  {t.message}
                                </UIText>
                              )}
                            </div>
                          </SelectCommandItem>
                        ))}
                      </SelectCommandGroup>
                    </>
                  )}
                </SelectCommandList>

                <SelectCommandSeparator alwaysRender />
                <div className='flex items-center justify-end p-2'>
                  <Button
                    onClick={() => {
                      setOpen(false)
                      router.push({
                        pathname: '/[org]/code/tags',
                        query: { org: org ?? '' }
                      })
                    }}
                  >
                    View all tags
                  </Button>
                  {refs && (
                    <Button
                      className='ml-2'
                      variant='plain'
                      onClick={() => {
                        setOpen(false)
                        router.push(`/${org}/code/tree/main/${path}`)
                      }}
                    >
                      Clear
                    </Button>
                  )}
                </div>
              </SelectCommandContainer>
            </div>
          </PopoverContent>
        </PopoverPortal>
      </Popover>
    </>
  )
}
