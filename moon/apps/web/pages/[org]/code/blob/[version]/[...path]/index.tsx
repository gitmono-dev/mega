import React, { useCallback, useEffect, useRef, useState } from 'react'
import { Theme } from '@radix-ui/themes'
import { useParams } from 'next/navigation'

import CodeContent from '@/components/CodeView/BlobView/CodeContent'
import CommitHistory from '@/components/CodeView/CommitHistory'
import BreadCrumb from '@/components/CodeView/TreeView/BreadCrumb'
import RepoTree from '@/components/CodeView/TreeView/RepoTree'
import { AppLayout } from '@/components/Layout/AppLayout'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { useGetBlob } from '@/hooks/useGetBlob'

const MIN_LEFT_WIDTH = 250
const MAX_LEFT_WIDTH_PERCENT = 0.4 // Reduced from 0.5 to 0.4 to ensure right side has enough space
const DEFAULT_LEFT_WIDTH_PERCENT = 0.2
const MIN_RIGHT_WIDTH = 500 // Add minimum width constraint for right side

function BlobPage() {
  const params = useParams()
  const { version, path = [] } = params as { version: string; path?: string[] }

  const refs = version === 'main' ? undefined : version
  const new_path = '/' + path.join('/')

  const { data: blobData, isLoading: isCodeLoading } = useGetBlob({ path: new_path, refs })
  const fileContent = blobData?.data ?? ''

  // Resizable panel state
  const containerRef = useRef<HTMLDivElement>(null)
  const [leftWidth, setLeftWidth] = useState<number | null>(null)
  const [isDragging, setIsDragging] = useState(false)

  // Initialize left width based on container width
  useEffect(() => {
    if (containerRef.current && leftWidth === null) {
      setLeftWidth(containerRef.current.offsetWidth * DEFAULT_LEFT_WIDTH_PERCENT)
    }
  }, [leftWidth])

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }, [])

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      if (!isDragging || !containerRef.current) return

      const containerRect = containerRef.current.getBoundingClientRect()
      const newLeftWidth = e.clientX - containerRect.left
      const maxWidth = containerRect.width * MAX_LEFT_WIDTH_PERCENT

      // Ensure right side has at least MIN_RIGHT_WIDTH width
      const maxAllowedLeftWidth = Math.min(maxWidth, containerRect.width - MIN_RIGHT_WIDTH)

      setLeftWidth(Math.max(MIN_LEFT_WIDTH, Math.min(newLeftWidth, maxAllowedLeftWidth)))
    },
    [isDragging]
  )

  const handleMouseUp = useCallback(() => {
    setIsDragging(false)
  }, [])

  useEffect(() => {
    if (isDragging) {
      document.addEventListener('mousemove', handleMouseMove)
      document.addEventListener('mouseup', handleMouseUp)
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'
    }

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
  }, [isDragging, handleMouseMove, handleMouseUp])

  return (
    <Theme>
      <div className='relative m-4 flex h-[calc(100vh-32px)] flex-col'>
        <BreadCrumb path={path} />
        {/* File tree */}
        <div ref={containerRef} className='flex flex-1 gap-0 overflow-hidden'>
          <div
            style={{
              width: leftWidth ?? '20%',
              minWidth: MIN_LEFT_WIDTH,
              flexShrink: 0,
              borderRadius: 8,
              overflow: 'auto',
              paddingRight: '8px'
            }}
            className='bg-primary'
          >
            <RepoTree />
          </div>

          {/* Resizer handle */}
          <div
            onMouseDown={handleMouseDown}
            className='bg-border-primary h-full w-1 shrink-0 cursor-col-resize transition-colors hover:bg-blue-400'
            style={{ backgroundColor: isDragging ? '#60a5fa' : undefined }}
          />

          <div
            style={{
              flex: 1,
              borderRadius: 8,
              display: 'flex',
              flexDirection: 'column',
              overflow: 'hidden',
              paddingLeft: '8px',
              paddingRight: '8px'
            }}
            className='bg-primary'
          >
            <div className='shrink-0'>
              <CommitHistory flag={'details'} path={new_path} refs={refs} />
            </div>
            <div className='flex flex-1 flex-col overflow-hidden pt-2'>
              <CodeContent fileContent={fileContent} path={path} isCodeLoading={isCodeLoading} />
            </div>
          </div>
        </div>
      </div>
    </Theme>
  )
}

BlobPage.getProviders = (
  page: React.ReactNode,
  pageProps: React.JSX.IntrinsicAttributes & { children?: React.ReactNode }
) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default BlobPage
