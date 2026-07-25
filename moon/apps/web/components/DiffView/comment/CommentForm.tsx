import { useCallback, useEffect, useMemo, useRef } from 'react'
import type { AnnotationSide, FileDiffMetadata, SelectedLineRange } from '@pierre/diffs'

import { DiffSide } from '@gitmono/types/generated'
import { Avatar, Button } from '@gitmono/ui'

import { useGetClFilesList } from '@/hooks/CL/useGetClFilesList'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'

import { useInitComment } from '../hooks/useInitComment'
import { getLineContext } from './utils'

interface CommentFormProps {
  side: AnnotationSide
  lineNumber: number
  filePath: string
  fileDiff: FileDiffMetadata
  selectedRange: SelectedLineRange | null
  clLink: string
  onSubmit: () => void
  onCancel: () => void
}

export function CommentForm({
  side,
  lineNumber,
  filePath,
  fileDiff,
  selectedRange,
  clLink,
  onSubmit,
  onCancel
}: CommentFormProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const { data: currentUser } = useGetCurrentUser()
  const { mutate: initComment, isPending } = useInitComment()

  const { data: filesListData } = useGetClFilesList(clLink)

  const currentUserAvatarUrl = useMemo(() => currentUser?.avatar_url || null, [currentUser])

  const currentUserName = useMemo(() => currentUser?.display_name || currentUser?.username || 'You', [currentUser])

  const commitSha = useMemo(() => {
    if (!filesListData?.data) return 'HEAD'
    const fileInfo = filesListData.data.find((file) => file.path.endsWith(filePath))

    return fileInfo?.sha || 'HEAD'
  }, [filesListData, filePath])

  const { startLine, endLine } = useMemo(() => {
    if (!selectedRange) {
      return { startLine: lineNumber, endLine: lineNumber }
    }

    const start = Math.min(selectedRange.start, selectedRange.end)
    const end = Math.max(selectedRange.start, selectedRange.end)

    return { startLine: start, endLine: end }
  }, [selectedRange, lineNumber])

  const lineContext = useMemo(() => {
    return getLineContext(fileDiff, side, startLine, endLine, 1)
  }, [fileDiff, side, startLine, endLine])

  useEffect(() => {
    setTimeout(() => {
      textareaRef.current?.focus()
    }, 0)
  }, [])

  const handleSubmit = useCallback(() => {
    const content = textareaRef.current?.value.trim()

    if (!content || !lineContext) return

    initComment(
      {
        link: clLink,
        data: {
          anchor_commit_sha: commitSha,
          content,
          normalized_content: lineContext.normalizedContent,
          context_after: lineContext.contextAfter.join('\n'),
          context_before: lineContext.contextBefore.join('\n'),
          diff_side: side === 'deletions' ? DiffSide.Deletions : DiffSide.Additions,
          file_path: filePath,
          original_line_number: endLine
        }
      },
      {
        onSuccess: () => {
          onSubmit()
          if (textareaRef.current) {
            textareaRef.current.value = ''
          }
        }
      }
    )
  }, [side, endLine, filePath, clLink, commitSha, lineContext, onSubmit, initComment])

  const handleCancel = useCallback(() => onCancel(), [onCancel])

  return (
    <div className='flex flex-row gap-0.5 overflow-hidden'>
      <div className='w-full'>
        <div className='mx-4 my-4 max-w-[95%] font-[Geist] whitespace-normal sm:max-w-[70%]'>
          <div className='rounded-lg border border-[#d0d7de] bg-white p-4 shadow-xs dark:border-[#30363d] dark:bg-[#0d1117]'>
            <div className='flex gap-3'>
              <div className='relative shrink-0'>
                <Avatar src={currentUserAvatarUrl} alt={currentUserName} name={currentUserName} size='sm' />
              </div>
              <div className='flex-1'>
                <textarea
                  ref={textareaRef}
                  placeholder='Leave a comment'
                  className='min-h-[80px] w-full resize-none rounded-md border border-[#d0d7de] bg-white p-3 font-[inherit] text-sm text-[#24292f] outline-hidden transition-colors focus:ring-2 dark:border-[#30363d] dark:bg-[#0d1117] dark:text-[#e6edf3]'
                />
                <div className='mt-3 flex items-center justify-end gap-2'>
                  <button
                    onClick={handleCancel}
                    className='rounded-md bg-transparent px-3 py-1 text-xs font-medium text-[#57606a] transition-colors hover:bg-[#f6f8fa] dark:text-[#8b949e] dark:hover:bg-[#21262d]'
                  >
                    Cancel
                  </button>
                  <Button
                    size='sm'
                    onClick={handleSubmit}
                    disabled={isPending}
                    className='bg-green-600 text-xs font-medium text-white hover:bg-green-700'
                  >
                    {isPending ? 'Submitting...' : 'Comment'}
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
