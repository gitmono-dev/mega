import { useState } from 'react'
import { toast } from 'react-hot-toast'

import type { CommentReviewResponse, ThreadReviewResponse } from '@gitmono/types/generated'
import { Avatar, Button } from '@gitmono/ui'

import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle } from '@/utils/megaUser'

import { useDeleteComment } from '../hooks/useDeleteComment'
import { useDeleteThread } from '../hooks/useDeleteThread'
import { useReopenThread } from '../hooks/useReopenThread'
import { useReplyComment } from '../hooks/useReplyComment'
import { useResolveThread } from '../hooks/useResolveThread'
import { useUpdateComment } from '../hooks/useUpdateComment'

function UserAvatar({ username, size = 'sm' }: { username: string; size?: 'xs' | 'sm' }) {
  const { data: member } = useMemberByActor(username)
  const displayName = megaUserHandle(member?.user, username)
  const avatarUrl = member?.user?.avatar_urls?.sm || member?.user?.avatar_urls?.base

  return <Avatar src={avatarUrl} alt={displayName} name={displayName} size={size} />
}

interface CommentItemProps {
  comment: CommentReviewResponse
  isMainComment?: boolean
  onEdit?: () => void
  onDelete?: () => void
  isEditing?: boolean
  editContent?: string
  onEditContentChange?: (content: string) => void
  onSaveEdit?: () => void
  onCancelEdit?: () => void
  isUpdating?: boolean
}

interface CommentThreadProps {
  thread: ThreadReviewResponse
  clLink: string
}

export function CommentThread({ thread, clLink }: CommentThreadProps) {
  const [isCollapsed, setIsCollapsed] = useState(thread.status === 'Resolved')
  const [showReplyInput, setShowReplyInput] = useState(false)
  const [replyContent, setReplyContent] = useState('')
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null)
  const [editContent, setEditContent] = useState('')
  const { data: currentUser } = useGetCurrentUser()

  const [mainComment, ...replies] = thread.comments
  const isResolved = thread.status === 'Resolved'
  const lineNumber = Math.abs(thread.position.line_number)
  const side = thread.anchor.diff_side === 'Deletions' ? 'L' : 'R'

  const { mutate: replyComment, isPending: isReplying } = useReplyComment(clLink)
  const { mutate: resolveThread, isPending: isResolving } = useResolveThread(clLink)
  const { mutate: reopenThread, isPending: isReopening } = useReopenThread(clLink)
  const { mutate: deleteThread } = useDeleteThread(clLink)
  const { mutate: deleteComment } = useDeleteComment(clLink)
  const { mutate: updateComment, isPending: isUpdating } = useUpdateComment(clLink)

  const handleReply = () => {
    if (!replyContent.trim()) return

    replyComment(
      {
        threadId: thread.thread_id,
        data: {
          content: replyContent,
          parent_comment_id: mainComment.comment_id
        }
      },
      {
        onSuccess: () => {
          setReplyContent('')
          setShowReplyInput(false)
        }
      }
    )
  }

  const handleResolve = () => {
    resolveThread(
      { threadId: thread.thread_id },
      {
        onSuccess: () => {
          toast.success('Thread 已解决')
          setIsCollapsed(true)
        },
        onError: (error) => toast.error(`解决失败: ${error.message}`)
      }
    )
  }

  const handleReopen = () => {
    reopenThread(
      { threadId: thread.thread_id },
      {
        onSuccess: () => {
          setIsCollapsed(false)
        }
      }
    )
  }

  const handleDeleteThread = () => {
    deleteThread({ threadId: thread.thread_id })
  }

  const handleDeleteComment = (commentId: string) => {
    deleteComment({ commentId })
  }

  const handleStartEdit = (commentId: string, currentContent: string) => {
    setEditingCommentId(commentId)
    setEditContent(currentContent)
  }

  const handleUpdateComment = (commentId: string) => {
    if (!editContent.trim()) return

    updateComment(
      {
        commentId,
        data: { content: editContent }
      },
      {
        onSuccess: () => {
          setEditingCommentId(null)
          setEditContent('')
        }
      }
    )
  }

  // 收缩状态
  if (isCollapsed) {
    return (
      <div className='mx-5 my-4 max-w-[95%] sm:max-w-[70%]'>
        <button
          onClick={() => setIsCollapsed(false)}
          className='flex w-full items-center justify-between rounded-lg border border-gray-300 bg-white px-3 py-2.5 text-left transition-colors hover:bg-gray-50 dark:border-gray-700 dark:bg-gray-900 dark:hover:bg-gray-800'
        >
          <div className='flex items-center gap-2'>
            <svg
              xmlns='http://www.w3.org/2000/svg'
              className='h-4 w-4 text-gray-500 dark:text-gray-400'
              fill='none'
              viewBox='0 0 24 24'
              stroke='currentColor'
            >
              <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M9 5l7 7-7 7' />
            </svg>
            <span className='text-sm text-gray-600 dark:text-gray-400'>
              Comment on line {side}
              {lineNumber}
            </span>
          </div>
          {isResolved && <span className='rounded-full border px-2 py-0.5 text-xs font-medium'>Resolved</span>}
        </button>
      </div>
    )
  }

  // 展开状态
  return (
    <div className='mx-5 my-4 max-w-[95%] sm:max-w-[70%]'>
      {/* 折叠指示器头部 */}
      <button
        onClick={() => setIsCollapsed(true)}
        className='dark:hover:bg-gray-750 flex w-full items-center justify-between rounded-t-lg border border-b-0 border-gray-300 bg-gray-50 px-3 py-2.5 text-left transition-colors hover:bg-gray-100 dark:border-gray-700 dark:bg-gray-800'
      >
        <div className='flex items-center gap-2'>
          <svg
            xmlns='http://www.w3.org/2000/svg'
            className='h-4 w-4 text-gray-500 dark:text-gray-400'
            fill='none'
            viewBox='0 0 24 24'
            stroke='currentColor'
          >
            <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M19 9l-7 7-7-7' />
          </svg>
          <span className='text-sm text-gray-600 dark:text-gray-400'>
            Comment on line {side}
            {lineNumber}
          </span>
        </div>
        {isResolved && <span className='rounded-full border px-2 py-0.5 text-xs font-medium'>Resolved</span>}
      </button>

      <div className='rounded-b-lg border border-gray-300 bg-white p-4 shadow-xs dark:border-gray-700 dark:bg-gray-900'>
        {/* 主评论 */}
        <CommentItem
          comment={mainComment}
          isMainComment
          isEditing={editingCommentId === mainComment.comment_id}
          editContent={editContent}
          onEditContentChange={setEditContent}
          onEdit={() => handleStartEdit(mainComment.comment_id, mainComment.content || '')}
          onDelete={handleDeleteThread}
          onSaveEdit={() => handleUpdateComment(mainComment.comment_id)}
          onCancelEdit={() => {
            setEditingCommentId(null)
            setEditContent('')
          }}
          isUpdating={isUpdating}
        />

        {/* 回复列表 */}
        {replies.length > 0 && (
          <div className='mt-4 ml-11 space-y-4 border-l-2 border-gray-200 pl-4 dark:border-gray-700'>
            {replies.map((reply) => (
              <CommentItem
                key={reply.comment_id}
                comment={reply}
                isEditing={editingCommentId === reply.comment_id}
                editContent={editContent}
                onEditContentChange={setEditContent}
                onEdit={() => handleStartEdit(reply.comment_id, reply.content || '')}
                onDelete={() => handleDeleteComment(reply.comment_id)}
                onSaveEdit={() => handleUpdateComment(reply.comment_id)}
                onCancelEdit={() => {
                  setEditingCommentId(null)
                  setEditContent('')
                }}
                isUpdating={isUpdating}
              />
            ))}
          </div>
        )}

        {/* 回复输入区域 */}
        <div className='mt-4 ml-11'>
          {showReplyInput ? (
            <div className='flex gap-3'>
              <Avatar src={currentUser?.avatar_url} alt={currentUser?.username} size='xs' />
              <div className='flex-1'>
                <textarea
                  value={replyContent}
                  onChange={(e) => setReplyContent(e.target.value)}
                  placeholder='Write a reply...'
                  className='min-h-[60px] w-full resize-none rounded-md border border-gray-300 bg-white p-2 text-xs text-gray-900 outline-hidden transition-colors focus:ring-2 focus:ring-blue-500 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100'
                  autoFocus
                />
                <div className='mt-2 flex items-center justify-end gap-2'>
                  <Button
                    onClick={() => {
                      setShowReplyInput(false)
                      setReplyContent('')
                    }}
                    variant='base'
                    size='sm'
                  >
                    Cancel
                  </Button>
                  <Button
                    onClick={handleReply}
                    disabled={isReplying}
                    variant='primary'
                    className='bg-[#1f883d]'
                    size='sm'
                  >
                    {isReplying ? 'Replying...' : 'Reply'}
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowReplyInput(true)}
              className='w-full rounded-md border border-gray-300 px-3 py-2 text-left text-sm text-gray-500 transition-colors hover:border-gray-400 dark:border-gray-700 dark:text-gray-400 dark:hover:border-gray-600'
            >
              Write a reply
            </button>
          )}
        </div>

        {/* Resolve/Reopen 按钮区域 */}
        <div className='mt-4 flex border-t border-gray-200 pt-4 dark:border-gray-700'>
          {isResolved ? (
            <Button onClick={handleReopen} disabled={isReopening} variant='base' size='sm'>
              {isReopening ? 'Reopening...' : 'Reopen conversation'}
            </Button>
          ) : (
            <Button onClick={handleResolve} disabled={isResolving} variant='base' size='sm'>
              {isResolving ? 'Resolving...' : 'Resolve conversation'}
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
function CommentItem({
  comment,
  isMainComment = false,
  onEdit,
  onDelete,
  isEditing,
  editContent,
  onEditContentChange,
  onSaveEdit,
  onCancelEdit,
  isUpdating
}: CommentItemProps) {
  const [showMenu, setShowMenu] = useState(false)
  const { data: member } = useMemberByActor(comment.user_name)
  const displayName = megaUserHandle(member?.user, comment.user_name)

  return (
    <div className='flex gap-3'>
      <UserAvatar username={comment.user_name} size={isMainComment ? 'sm' : 'xs'} />
      <div className='min-w-0 flex-1'>
        <div className='mb-1 flex items-center justify-between'>
          <div className='flex items-center gap-2'>
            <span className='text-sm font-semibold text-gray-900 dark:text-gray-100'>{displayName}</span>
            <span className='text-xs text-gray-500 dark:text-gray-400'>{comment.created_at}</span>
          </div>
          {(onEdit || onDelete) && (
            <div className='relative'>
              <button
                onClick={() => setShowMenu(!showMenu)}
                className='rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700 dark:text-gray-400 dark:hover:bg-gray-800 dark:hover:text-gray-300'
                title='More options'
              >
                <svg
                  xmlns='http://www.w3.org/2000/svg'
                  className='h-4 w-4'
                  fill='none'
                  viewBox='0 0 24 24'
                  stroke='currentColor'
                >
                  <path
                    strokeLinecap='round'
                    strokeLinejoin='round'
                    strokeWidth={2}
                    d='M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z'
                  />
                </svg>
              </button>
              {showMenu && (
                <>
                  <div className='fixed inset-0 z-10' onClick={() => setShowMenu(false)} />
                  <div className='absolute top-8 right-0 z-20 w-32 rounded-md border border-gray-200 bg-white py-1 shadow-lg dark:border-gray-700 dark:bg-gray-800'>
                    {onEdit && (
                      <button
                        onClick={() => {
                          onEdit()
                          setShowMenu(false)
                        }}
                        className='flex w-full items-center px-3 py-2 text-left text-sm text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700'
                      >
                        Edit
                      </button>
                    )}
                    {onDelete && (
                      <button
                        onClick={() => {
                          onDelete()
                          setShowMenu(false)
                        }}
                        className='flex w-full items-center px-3 py-2 text-left text-sm text-red-600 hover:bg-gray-100 dark:text-red-400 dark:hover:bg-gray-700'
                      >
                        Delete
                      </button>
                    )}
                  </div>
                </>
              )}
            </div>
          )}
        </div>

        {isEditing ? (
          <div>
            <textarea
              value={editContent}
              onChange={(e) => onEditContentChange?.(e.target.value)}
              className='min-h-[60px] w-full resize-none rounded-md border border-gray-300 bg-white p-2 text-sm text-gray-900 outline-hidden transition-colors focus:ring-2 focus:ring-blue-500 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100'
            />
            <div className='mt-2 flex justify-end gap-2'>
              <Button onClick={onCancelEdit} variant='base' size='sm'>
                Cancel
              </Button>
              <Button onClick={onSaveEdit} disabled={isUpdating} variant='primary' className='bg-[#1f883d]' size='sm'>
                {isUpdating ? 'Save...' : 'Save'}
              </Button>
            </div>
          </div>
        ) : (
          <div className='text-sm leading-relaxed text-gray-900 dark:text-gray-100'>{comment.content}</div>
        )}
      </div>
    </div>
  )
}
