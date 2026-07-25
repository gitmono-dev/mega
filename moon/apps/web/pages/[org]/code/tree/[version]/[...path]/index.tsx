'use client'

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Theme } from '@radix-ui/themes'
import { useParams } from 'next/navigation'

import { CommonResultVecTreeCommitItem } from '@gitmono/types/generated'
import { Button } from '@gitmono/ui/Button'
import { CloseIcon } from '@gitmono/ui/Icons'

import CodeTable from '@/components/CodeView/CodeTable'
import CommitHistory from '@/components/CodeView/CommitHistory'
import NewCodeView from '@/components/CodeView/NewCodeView/NewCodeView'
import TagSwitcher from '@/components/CodeView/Tags/TagSwitcher'
import BreadCrumb from '@/components/CodeView/TreeView/BreadCrumb'
import CloneTabs from '@/components/CodeView/TreeView/CloneTabs'
import RepoTree from '@/components/CodeView/TreeView/RepoTree'
import SyncRepoButton from '@/components/CodeView/TreeView/SyncRepoButton'
import { canShowGitHubSync } from '@/components/CodeView/TreeView/syncUtils'
import { AppLayout } from '@/components/Layout/AppLayout'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { useGetBlob } from '@/hooks/useGetBlob'
import { useGetTreeCommitInfo } from '@/hooks/useGetTreeCommitInfo'
import { useGetTreePathCanClone } from '@/hooks/useGetTreePathCanClone'

const MIN_LEFT_WIDTH = 250
const MAX_LEFT_WIDTH_PERCENT = 0.4
const DEFAULT_LEFT_WIDTH_PERCENT = 0.2
const MIN_RIGHT_WIDTH = 500

function TreeDetailPage() {
  const params = useParams()
  const { version, path = [] } = params as { version: string; path?: string[] }

  const refs = version === 'main' ? undefined : version
  const new_path = '/' + path?.join('/')

  const [newPath, setNewPath] = useState(new_path)
  const { data: TreeCommitInfo } = useGetTreeCommitInfo(newPath, refs)

  type DirectoryType = NonNullable<CommonResultVecTreeCommitItem['data']>
  const directory: DirectoryType = useMemo(() => TreeCommitInfo?.data ?? [], [TreeCommitInfo])

  const { data: canClone } = useGetTreePathCanClone({ path: newPath })

  const reqPath = `${new_path}/README.md`
  const { data: readmeContent } = useGetBlob({ path: reqPath, refs })

  const [isNewCode, setIsNewCode] = useState(false)
  const [newEntryType, setNewEntryType] = useState<'file' | 'folder'>('file')

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

  const treeStyle = {
    borderRadius: 8,
    width: leftWidth ?? '20%',
    minWidth: MIN_LEFT_WIDTH,
    flexShrink: 0,
    height: 'calc(100vh - 96px)',
    overflow: 'auto',
    paddingRight: '8px'
  }

  const codeStyle = {
    borderRadius: 8,
    flex: 1,
    height: 'calc(100vh - 96px)',
    overflow: 'auto',
    paddingLeft: '8px',
    paddingRight: '8px'
  }

  const handleNewClick = (type: 'file' | 'folder') => {
    setNewEntryType(type)
    setIsNewCode(true)
  }
  const handleCloseClick = () => {
    setIsNewCode(false)
  }

  return (
    <Theme>
      <div className='relative m-4 h-screen'>
        <div className='flex min-h-12 items-center justify-between'>
          <BreadCrumb path={path} />
          {!isNewCode ? (
            <>
              <div className='m-1 flex justify-end gap-2'>
                <TagSwitcher />
                {version == 'main' && path[0] !== 'third-party' && (
                  <Button onClick={() => handleNewClick('file')}>New File</Button>
                )}
                {version == 'main' && path[0] !== 'third-party' && (
                  <Button onClick={() => handleNewClick('folder')}>New Folder</Button>
                )}
                {canClone?.data && <CloneTabs />}
                {canShowGitHubSync(path, version) && <SyncRepoButton currentPath={newPath} />}
              </div>
            </>
          ) : (
            <Button onClick={handleCloseClick}>
              <CloseIcon />
            </Button>
          )}
        </div>
        <div ref={containerRef} className='flex h-full gap-0'>
          <div style={treeStyle} className='bg-primary'>
            <RepoTree onCommitInfoChange={(path: string) => setNewPath(path)} />
          </div>

          {/* Resizer handle */}
          <div
            onMouseDown={handleMouseDown}
            className='bg-border-primary h-full w-1 shrink-0 cursor-col-resize transition-colors hover:bg-blue-400'
            style={{ backgroundColor: isDragging ? '#60a5fa' : undefined }}
          />

          {!isNewCode ? (
            <div style={codeStyle} className='bg-primary'>
              <div>
                <CommitHistory flag={'contents'} path={newPath} refs={refs} />
              </div>
              <CodeTable directory={directory} loading={!TreeCommitInfo} readmeContent={readmeContent?.data} />
            </div>
          ) : (
            <div className='bg-primary flex-1 overflow-hidden pb-18'>
              <NewCodeView currentPath={new_path} onClose={handleCloseClick} defaultType={newEntryType} />
            </div>
          )}
        </div>
      </div>
    </Theme>
  )
}

TreeDetailPage.getProviders = (
  page: React.ReactNode,
  pageProps: React.JSX.IntrinsicAttributes & { children?: React.ReactNode | undefined }
) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default TreeDetailPage
