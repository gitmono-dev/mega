import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { getNoteExtensions } from '@gitmono/editor'
import { ConversationItem, ReviewerInfo } from '@gitmono/types/generated'
import { Button, ConditionalWrap, FaceSmilePlusIcon, PicturePlusIcon, UIText } from '@gitmono/ui'

import { EMPTY_HTML } from '@/atoms/markdown'
import { RichTextRenderer } from '@/components/RichTextRenderer'
import { usePostClReviewResolve } from '@/hooks/CL/usePostClReviewResolve'
import { usePostComment } from '@/hooks/issues/usePostComment'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { useUploadHelpers } from '@/hooks/useUploadHelpers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { megaUserHandle, megaUserHandlesMatch } from '@/utils/megaUser'
import { legacyApiClient } from '@/utils/queryClient'
import { trimHtml } from '@/utils/trimHtml'

import { ActorAvatar } from '../ActorAvatar'
import { BotBadge } from '../BotBadge'
import { MemberHovercard } from '../InlinePost/MemberHovercard'
import { useChange } from '../Issues/utils/sideEffect'
import { editIdAtom, FALSE_EDIT_VAL, refreshAtom } from '../Issues/utils/store'
import { useHandleBottomScrollOffset } from '../NoteEditor/useHandleBottomScrollOffset'
import { ComposerReactionPicker } from '../Reactions/ComposerReactionPicker'
import { ReactionPicker } from '../Reactions/ReactionPicker'
import { ReactionShow } from '../Reactions/ReactionShow'
import { SimpleNoteContent, SimpleNoteContentRef } from '../SimpleNoteEditor/SimpleNoteContent'
import { CommentDropdownMenu } from './CommentDropdownMenu'
import HandleTime from './components/HandleTime'
import { UserLinkByName } from './components/UserLinkByName'
import { useHandleExpression } from './hook/useHandleExpression'

interface ReviewCommentProps {
  reviewers: ReviewerInfo[]
  conv: ConversationItem
  id: string
  whoamI: string
  editorRef: React.RefObject<SimpleNoteContentRef | null>
}

const ReviewComment = React.memo<ReviewCommentProps>(
  ({ reviewers, conv, id, whoamI, editorRef }: ReviewCommentProps) => {
    const isBot = !!conv.is_bot
    const { data: member } = useMemberByActor(conv.username, !isBot)
    const { data: currentUser } = useGetCurrentUser()
    const profileUsername = member?.user.username || conv.username
    const displayName = megaUserHandle(member?.user, conv.username) || conv.username
    const { mutate: resolveReview } = usePostClReviewResolve()
    const queryClient = useQueryClient()
    const router = useRouter()
    const { link } = router.query
    const clLink = typeof link === 'string' ? link : ''

    const extensions = useMemo(() => getNoteExtensions({ linkUnfurl: {} }), [])
    const handleReactionSelect = useHandleExpression({ conv, id, type: whoamI })
    const [editId, setEditId] = useAtom(editIdAtom)
    const editInputRef = useRef<{ handleUpdate: () => void } | null>(null)
    const [isResolved, setIsResolved] = useState(conv.resolved ?? false)
    const [refresh, setRefresh] = useAtom(refreshAtom)

    const canResolve = useMemo(() => {
      if (!currentUser) {
        return false
      }
      return reviewers.some((r) => megaUserHandlesMatch(r.campsite_user_id || r.username, currentUser))
    }, [currentUser, reviewers])

    useEffect(() => {
      setIsResolved(conv.resolved ?? false)
    }, [conv.resolved])

    const handleResolve = useCallback(async () => {
      try {
        await new Promise<void>((resolve, reject) => {
          resolveReview(
            {
              link: clLink,
              data: {
                conversation_id: conv.id,
                resolved: true
              }
            },
            {
              onSuccess: () => {
                setIsResolved(true)
                toast.success('Review resolved successfully!')

                queryClient.invalidateQueries({
                  queryKey: legacyApiClient.v1.getApiClDetail().requestKey(id)
                })

                resolve()
              },
              onError: (error: any) => {
                apiErrorToast(new Error(error.message))
                reject(error)
              }
            }
          )
        })
      } catch (error) {
        // Error already handled in onError callback
      }
    }, [resolveReview, clLink, conv.id, queryClient, id])

    return (
      <div
        className={`overflow-hidden rounded-lg border-2 ${
          isResolved
            ? 'border-green-200 bg-green-50 dark:border-green-800 dark:bg-green-950'
            : 'border-blue-200 bg-blue-50 dark:border-blue-800 dark:bg-blue-950'
        }`}
      >
        <div className='bg-blue-100 px-3 py-1 text-xs font-medium text-blue-800 dark:bg-blue-900 dark:text-blue-200'>
          📝 Review Comment
          {isResolved && (
            <span className='ml-2 inline-flex items-center rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-800 dark:bg-green-900 dark:text-green-200'>
              ✓ Resolved
            </span>
          )}
        </div>

        <div className='border-primary flex items-center justify-between border-b px-4 py-2'>
          <div className='flex items-center space-x-3'>
            <div className='cursor-pointer'>
              <ConditionalWrap
                condition={!isBot}
                wrap={(c) => (
                  <MemberHovercard username={profileUsername}>
                    <UserLinkByName username={profileUsername} className='relative'>
                      {c}
                    </UserLinkByName>
                  </MemberHovercard>
                )}
              >
                <ActorAvatar member={member} username={conv.username} isBot={isBot} size='sm' />
              </ConditionalWrap>
            </div>
            <div className='flex items-center gap-1.5'>
              <ConditionalWrap
                condition={!isBot}
                wrap={(children) => (
                  <MemberHovercard username={profileUsername}>
                    <UserLinkByName username={profileUsername}>{children}</UserLinkByName>
                  </MemberHovercard>
                )}
              >
                <UIText element='span' primary weight='font-medium' className='break-anywhere line-clamp-1'>
                  {displayName}
                </UIText>
              </ConditionalWrap>
              {isBot && <BotBadge size='sm' />}
            </div>
            <div className='text-tertiary hover:text-secondary text-sm'>
              <HandleTime created_at={conv.created_at} />
            </div>
          </div>
          <div className='flex items-center space-x-2'>
            {canResolve && !isResolved && (
              <Button
                size='sm'
                variant='base'
                onClick={handleResolve}
                className='border-green-300 text-green-600 hover:bg-green-50 dark:border-green-700 dark:text-green-400 dark:hover:bg-green-950'
              >
                Resolve Conversation
              </Button>
            )}
            <ReactionPicker
              custom
              align='end'
              trigger={
                <Button
                  variant='plain'
                  iconOnly={<FaceSmilePlusIcon />}
                  accessibilityLabel='Add reaction'
                  tooltip='Add reaction'
                />
              }
              onReactionSelect={handleReactionSelect}
            />
            <CommentDropdownMenu id={id} Conversation={conv} CommentType={whoamI} editorRef={editorRef} />
          </div>
        </div>

        <div className='prose copyable-text p-3'>
          {conv.comment && editId !== conv.id && <RichTextRenderer content={conv.comment} extensions={extensions} />}
          {editId === conv.id && (
            <>
              <EditInput ref={editInputRef} comment={conv} />
              <div className='flex justify-end gap-4'>
                <Button disabled={refresh !== 0} onClick={() => setEditId(FALSE_EDIT_VAL)}>
                  Cancel
                </Button>
                <Button
                  disabled={refresh !== 0}
                  onClick={() => {
                    setRefresh(Date.now())
                    editInputRef.current && editInputRef.current.handleUpdate()
                  }}
                  className='bg-[#1f883d] text-white'
                >
                  Update Comment
                </Button>
              </div>
            </>
          )}
          <ReactionShow comment={conv} id={id} type={whoamI} />
        </div>
      </div>
    )
  }
)

ReviewComment.displayName = 'ReviewComment'

const EditInput = forwardRef(({ comment }: { comment: ConversationItem }, ref) => {
  const editorRef = useRef<SimpleNoteContentRef>(null)
  const onKeyDownScrollHandler = useHandleBottomScrollOffset({
    editor: editorRef.current?.editor
  })
  const { dropzone } = useUploadHelpers({
    upload: editorRef.current?.uploadAndAppendAttachments
  })
  const { handleChange } = useChange({})
  const [isReactionPickerOpen, setIsReactionPickerOpen] = useState(false)
  const { mutateAsync: updateComment } = usePostComment()
  const [isUpdating, setIsUpdating] = useState(false)

  const handleUpdate = async () => {
    if (isUpdating) return

    const currentContentHTML = editorRef.current?.editor?.getHTML() ?? '<p></p>'

    if (trimHtml(currentContentHTML) === '') {
      toast.error('comment can not be empty!')
      return
    }

    setIsUpdating(true)
    try {
      await updateComment({ commentId: comment.id, data: { content: currentContentHTML } })
      toast.success('update successfully!')
    } catch (err: any) {
      apiErrorToast(new Error(err.message))
    } finally {
      setIsUpdating(false)
    }
  }

  useImperativeHandle(ref, () => ({
    handleUpdate
  }))

  return (
    <>
      <div className='prose mt-4 flex w-full flex-col'>
        <input {...dropzone.getInputProps()} />
        <div className='relative rounded-lg border p-6 pb-12'>
          <SimpleNoteContent
            commentId='temp'
            ref={editorRef}
            editable='all'
            content={comment.comment || EMPTY_HTML}
            autofocus={true}
            onKeyDown={onKeyDownScrollHandler}
            onChange={(html) => handleChange(html)}
          />
          <Button
            variant='plain'
            iconOnly={<PicturePlusIcon />}
            accessibilityLabel='Add files'
            onClick={dropzone.open}
            tooltip='Add files'
          />
          <ComposerReactionPicker
            editorRef={editorRef}
            open={isReactionPickerOpen}
            onOpenChange={setIsReactionPickerOpen}
          />
        </div>
      </div>
    </>
  )
})

EditInput.displayName = 'EditInput'

export default ReviewComment
