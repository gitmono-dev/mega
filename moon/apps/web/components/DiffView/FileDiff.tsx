import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type ChangeTypes, type FileDiffMetadata } from '@pierre/diffs'
import { FileDiff as PierreFileDiff } from '@pierre/diffs/react'
import { useTheme } from 'next-themes'
import { useRouter } from 'next/router'
import { Virtuoso } from 'react-virtuoso'

import { CommonPageDiffItemSchema, CommonResultCodeReviewResponse, CommonResultVecMuiTreeNode } from '@gitmono/types'
import { LoadingSpinner } from '@gitmono/ui'
import { ExpandIcon, SparklesIcon } from '@gitmono/ui/Icons'
import { cn } from '@gitmono/ui/src/utils'

import FileTree from '@/components/DiffView/TreeView/FileTree'

import { CommentForm, CommentThread, HoverButton, useComments } from './comment'
import { useGetComments } from './hooks/useGetComments'
import { DiffItem, generateParsedFiles, parsedDiffs } from './parsedDiffs'

interface FileDiffProps {
  fileChangeData: CommonPageDiffItemSchema
  fileChangeIsLoading: boolean
  treeData: CommonResultVecMuiTreeNode['data']
  treeIsLoading: boolean
  page: number
  hasMoreData: boolean
  isBlockingLoading: boolean
  onLoadMore: () => void
}

function SingleFileDiffView({
  filePath,
  fileDiffMetadata,
  changeType,
  isBinary,
  hasContent,
  clLink,
  commentsData
}: {
  filePath: string
  fileDiffMetadata: FileDiffMetadata | null
  changeType: ChangeTypes | null
  isBinary: boolean
  hasContent: boolean
  clLink: string
  commentsData?: CommonResultCodeReviewResponse
}) {
  const { resolvedTheme } = useTheme()

  const {
    annotations,
    selectedRange,
    handleLineSelectionEnd,
    addCommentAtLine,
    handleSubmitComment,
    handleCancelComment
  } = useComments(filePath, commentsData?.data)

  const getChangeTypeMessage = (changeType: ChangeTypes | null): string | null => {
    switch (changeType) {
      case 'new':
        return 'This file was added.'
      case 'deleted':
        return 'This file was deleted.'
      case 'rename-pure':
        return 'This file was renamed.'
      case 'rename-changed':
        return 'This file was renamed and modified.'
      default:
        return null
    }
  }

  const message = getChangeTypeMessage(changeType)

  if (fileDiffMetadata && hasContent) {
    return (
      <PierreFileDiff
        fileDiff={fileDiffMetadata}
        lineAnnotations={annotations}
        selectedLines={selectedRange}
        renderAnnotation={(annotation) =>
          annotation.metadata ? (
            <CommentThread thread={annotation.metadata} clLink={clLink} />
          ) : (
            <CommentForm
              side={annotation.side}
              lineNumber={annotation.lineNumber}
              filePath={filePath}
              fileDiff={fileDiffMetadata}
              selectedRange={selectedRange}
              clLink={clLink}
              onSubmit={handleSubmitComment}
              onCancel={handleCancelComment}
            />
          )
        }
        renderGutterUtility={(getHoveredLine) => (
          <HoverButton getHoveredLine={getHoveredLine} onAddComment={addCommentAtLine} />
        )}
        options={{
          theme: resolvedTheme === 'dark' ? 'min-dark' : 'min-light',
          diffStyle: 'unified',
          diffIndicators: 'classic',
          overflow: 'wrap',
          disableFileHeader: true,
          enableLineSelection: true,
          enableGutterUtility: true,
          onLineSelectionEnd: handleLineSelectionEnd,
          unsafeCSS: `
              ::-webkit-scrollbar { display: none !important; }
              code { padding: 0 !important; }
            `
        }}
        style={{ '--diffs-font-size': '14px' } as React.CSSProperties}
      />
    )
  }

  if (isBinary) {
    return (
      <div className='p-4 text-center'>
        <div className='text-primary'>Binary file</div>
        {message && <div className='text-secondary mt-1 text-sm'>{message}</div>}
      </div>
    )
  }

  if (message) {
    return (
      <div className='p-4 text-center'>
        <div className='text-primary'>Load Diff</div>
        <div className='text-secondary mt-1 text-sm'>{message}</div>
      </div>
    )
  }

  if (!hasContent) {
    return <div className='text-tertiary p-4 text-center'>No content change</div>
  }

  return null
}

export default function FileDiff({
  fileChangeData,
  fileChangeIsLoading,
  treeData,
  treeIsLoading,
  page,
  hasMoreData,
  isBlockingLoading,
  onLoadMore
}: FileDiffProps) {
  const virtuosoRef = useRef<any>(null)
  const router = useRouter()
  const { link } = router.query
  const clLink = typeof link === 'string' ? link : ''

  const { data: commentsData } = useGetComments(clLink)

  const [pageDataMap, setPageDataMap] = useState<Map<number, DiffItem[]>>(new Map())

  useEffect(() => {
    if (fileChangeData?.items && !fileChangeIsLoading) {
      setPageDataMap((prev) => {
        const newMap = new Map(prev)

        if (!newMap.has(page)) {
          newMap.set(page, fileChangeData.items)
        }
        return newMap
      })
    }
  }, [fileChangeData, fileChangeIsLoading, page])

  const fileDiff = useMemo(() => {
    const allItems: DiffItem[] = []

    for (let i = 1; i <= page; i++) {
      const pageData = pageDataMap.get(i)

      if (pageData) {
        allItems.push(...pageData)
      }
    }
    return allItems
  }, [pageDataMap, page])

  const diffFiles = useMemo(() => parsedDiffs(fileDiff), [fileDiff])

  const parsedFiles = useMemo(() => generateParsedFiles(diffFiles), [diffFiles])

  const [expandedMap, setExpandedMap] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(diffFiles.map((f) => [f.path, false]))
  )

  const fileRefs = useRef<Record<string, HTMLDivElement | null>>({})

  const toggleExpanded = (path: string) => {
    setExpandedMap((prev) => ({ ...prev, [path]: !prev[path] }))
  }

  const scrollToFile = useCallback(
    (filePath: string) => {
      const index = parsedFiles.findIndex((pf) => pf.file.path === filePath)

      if (index !== -1) {
        setExpandedMap((prev) => ({ ...prev, [filePath]: true }))
        if (virtuosoRef.current) {
          virtuosoRef.current.scrollToIndex(index)
        }
      }
    },
    [parsedFiles]
  )

  const loadMoreDiffs = useCallback(() => {
    if (fileChangeIsLoading || !hasMoreData) return
    onLoadMore()
  }, [fileChangeIsLoading, hasMoreData, onLoadMore])

  useEffect(() => {
    setExpandedMap(Object.fromEntries(diffFiles.map((f) => [f.path, false])))
  }, [diffFiles])

  const DiffItemComponent = (index: number) => {
    const { file, fileDiffMetadata, stats, changeType, isBinary, hasContent } = parsedFiles[index]
    const isExpanded = expandedMap[file.path]
    const isRenamed = changeType === 'rename-pure' || changeType === 'rename-changed'

    return (
      <div
        id={file.path}
        key={file.path}
        ref={(el) => void (fileRefs.current[file.path] = el)}
        className='border-primary mb-4 w-full rounded-lg border'
      >
        <div
          onClick={() => toggleExpanded(file.path)}
          className={cn(
            'text-primary flex cursor-pointer items-center justify-between px-4 py-2 text-sm',
            isExpanded && 'border-primary border-b'
          )}
        >
          <span className='flex items-center'>
            {isExpanded ? (
              <SparklesIcon className='align-middle text-xl' />
            ) : (
              <ExpandIcon className='align-middle text-xl' />
            )}
            <span className='ml-1'>
              {isRenamed && file.oldPath ? (
                <span>
                  <span className='text-secondary line-through'>{file.oldPath}</span>
                  <span className='text-secondary mx-1'>→</span>
                  <span>{file.path}</span>
                </span>
              ) : (
                file.path
              )}
            </span>
          </span>
          <span className='text-xs font-bold'>
            <span className='text-green-500'>+{stats.additions}</span>{' '}
            <span className='text-red-500'>−{stats.deletions}</span>
          </span>
        </div>

        <div className='copyable-text'>
          {isExpanded && (
            <SingleFileDiffView
              filePath={file.path}
              fileDiffMetadata={fileDiffMetadata}
              changeType={changeType}
              isBinary={isBinary}
              hasContent={hasContent}
              clLink={clLink}
              commentsData={commentsData}
            />
          )}
        </div>
      </div>
    )
  }

  return (
    <div className='relative mt-3 flex font-sans'>
      <div className='sticky top-5 h-[80vh] w-[300px] overflow-y-auto rounded-lg p-2'>
        <FileTree treeData={treeData} treeDataLoading={treeIsLoading} onFileClick={scrollToFile} />
      </div>

      <div className='relative h-full w-full flex-1 px-4'>
        {isBlockingLoading && (
          <div className='bg-primary/80 absolute inset-0 z-10 flex items-center justify-center rounded-lg'>
            <div className='bg-primary flex items-center rounded-md px-3 py-2 shadow-lg'>
              <LoadingSpinner />
              <span className='text-secondary ml-2 text-sm'>Loading diffs...</span>
            </div>
          </div>
        )}

        {fileDiff.length === 0 && !fileChangeIsLoading && page === 1 ? (
          <div className='text-primary flex h-[85vh] items-center justify-center'>No File Changed</div>
        ) : (
          <Virtuoso
            ref={virtuosoRef}
            style={{ height: '76vh', paddingBottom: '20px' }}
            totalCount={parsedFiles.length}
            itemContent={DiffItemComponent}
            endReached={loadMoreDiffs}
            components={{ Footer: () => null }}
            increaseViewportBy={350}
          />
        )}
      </div>
    </div>
  )
}
