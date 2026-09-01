'use client'

import React, { useEffect, useRef, useState } from 'react'
import {
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon
} from '@primer/octicons-react'
import { BaseStyles, ThemeProvider } from '@primer/react'
import { useAtom } from 'jotai'
import { useTheme } from 'next-themes'
import dynamic from 'next/dynamic'
import { useRouter } from 'next/router'
import { toast } from 'react-hot-toast'

import { CommonResultCLDetailRes } from '@gitmono/types/generated'

import { tabAtom } from '@/components/ClView/components/Checks/cpns/store'
import { ConversationTab } from '@/components/ClView/ConversationTab'
import { FileChangeTab } from '@/components/ClView/FileChangeTab'
import { useReviewerSelector } from '@/components/ClView/useReviewerSelector'
import TitleInput from '@/components/Issues/TitleInput'
import {
  useAssigneesSelector,
  useAvatars,
  useChange,
  useLabelMap,
  useLabels,
  useLabelsSelector,
  useMemberMap
} from '@/components/Issues/utils/sideEffect'
import { clIdAtom, editIdAtom, FALSE_EDIT_VAL, refreshAtom } from '@/components/Issues/utils/store'
import { AppLayout } from '@/components/Layout/AppLayout'
import { TabLayout } from '@/components/Layout/TabLayout'
import { useHandleBottomScrollOffset } from '@/components/NoteEditor/useHandleBottomScrollOffset'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { SimpleNoteContentRef } from '@/components/SimpleNoteEditor/SimpleNoteContent'
import { useScope } from '@/contexts/scope'
import { useDeleteClReviewers } from '@/hooks/CL/useDeleteClReviewers'
import { useGetClDetail } from '@/hooks/CL/useGetClDetail'
import { useGetClReviewers } from '@/hooks/CL/useGetClReviewers'
import { usePostCLAssignees } from '@/hooks/CL/usePostCLAssignees'
import { usePostClClose } from '@/hooks/CL/usePostClClose'
import { usePostClComment } from '@/hooks/CL/usePostClComment'
import { usePostCLLabels } from '@/hooks/CL/usePostCLLabels'
import { usePostClReopen } from '@/hooks/CL/usePostClReopen'
import { usePostClReviewers } from '@/hooks/CL/usePostClReviewers'
import { useUploadHelpers } from '@/hooks/useUploadHelpers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { trimHtml } from '@/utils/trimHtml'
import { PageWithLayout } from '@/utils/types'

const CLDetailPage: PageWithLayout<any> = () => {
  const router = useRouter()
  const { link: tempId } = router.query
  const { theme } = useTheme()

  const [item_id] = useAtom(clIdAtom)

  const { scope } = useScope()
  const [login, _setLogin] = useState(true)
  const [isReactionPickerOpen, setIsReactionPickerOpen] = useState(false)

  const id = typeof tempId === 'string' ? tempId : ''
  const { data: ClDetailData, isLoading: detailIsLoading, refetch } = useGetClDetail(id)
  const { reviewers, isLoading: ReviewIsLoading } = useGetClReviewers(id)

  const clDetail = ClDetailData?.data as CommonResultCLDetailRes['data'] | undefined
  const { closeHint, needComment, handleChange } = useChange({ title: 'Close Change List' })
  const { mutate: clAssignees } = usePostCLAssignees()
  const { mutate: clReviewers } = usePostClReviewers()
  const { mutate: clLabels } = usePostCLLabels()
  const Checks = dynamic(() => import('@/components/ClView/components/Checks'))

  const [_, setEditId] = useAtom(editIdAtom)
  const [refresh, setRefresh] = useAtom(refreshAtom)

  useEffect(() => {
    if (refresh === 0) return

    const load = async () => {
      await refetch()
      setEditId(FALSE_EDIT_VAL)
      setRefresh(0)
    }

    load()
  }, [refresh, refetch, setEditId, setRefresh])

  const { mutate: closeCl, isPending: clCloseIsPending } = usePostClClose(id)
  const handleClClose = () => {
    if (closeHint === 'Close with comment') {
      send_comment()
    }
    closeCl(undefined, {
      onSuccess: () => {
        router.push(`/${scope}/cl`)
      }
    })
  }

  const { mutate: reopenCl, isPending: clReopenIsPending } = usePostClReopen(id)
  const handleClReopen = () => {
    if (needComment.current) {
      send_comment()
      needComment.current = false
    }
    reopenCl(undefined, {
      onSuccess: () => {
        router.push(`/${scope}/cl`)
      }
    })
  }

  const { mutate: postClComment, isPending: clCommentIsPending } = usePostClComment(id)

  const send_comment = () => {
    const currentContentHTML = editorRef.current?.editor?.getHTML() ?? '<p></p>'
    const issues = editorRef.current?.getLinkedIssues() || []

    /* eslint-disable-next-line no-console */
    console.log('commentIssues:', issues)

    if (trimHtml(currentContentHTML) === '') {
      toast.error('Please enter the content.')
    } else {
      postClComment(
        { content: currentContentHTML },
        {
          onSuccess: () => {
            editorRef.current?.clearAndBlur()
          }
        }
      )
    }
  }

  const buttonClasses = 'cursor-pointer'
  const editorRef = useRef<SimpleNoteContentRef>(null)
  const onKeyDownScrollHandler = useHandleBottomScrollOffset({
    editor: editorRef.current?.editor
  })
  const { dropzone } = useUploadHelpers({
    upload: editorRef.current?.uploadAndAppendAttachments
  })

  const avatars = useAvatars({ preferGithubLogin: true })

  const memberMap = useMemberMap()

  const labels = useLabels()

  const labelMap = useLabelMap()

  const { open, handleAssignees, handleOpenChange, fetchSelected } = useAssigneesSelector({
    assignees: ClDetailData?.data?.assignees ?? [],
    assignRequest: (selected) =>
      clAssignees(
        {
          data: {
            link: id,
            item_id: item_id,
            assignees: selected
          }
        },
        {
          onSuccess: async () => {
            editorRef.current?.clearAndBlur()
            await refetch({ throwOnError: true })
          },
          onError: apiErrorToast
        }
      ),
    avatars
  })
  const {
    open: review_open,
    handleAssignees: handleReviewers,
    handleOpenChange: review_handleOpenChange,
    fetchSelected: review_fetchSelected,
    availableAvatars
  } = useReviewerSelector({
    reviewers,
    reviewRequest: (selected) =>
      clReviewers(
        {
          link: id,
          data: {
            reviewer_usernames: selected
          }
        },
        {
          onSuccess: async () => {
            editorRef.current?.clearAndBlur()
          },
          onError: apiErrorToast
        }
      ),
    avatars
  })

  const {
    open: label_open,
    handleLabels,
    handleOpenChange: label_handleOpenChange,
    fetchSelected: label_fetchSelected
  } = useLabelsSelector({
    labelList: labels,
    labels: ClDetailData?.data?.labels ?? [],
    updateLabelsRequest: (selected) => {
      clLabels(
        {
          data: {
            item_id: item_id,
            label_ids: selected,
            link: `${tempId}`
          }
        },
        {
          onSuccess: async () => {
            editorRef.current?.clearAndBlur()
            await refetch({ throwOnError: true })
          },
          onError: apiErrorToast
        }
      )
    }
  })

  const { mutate: deleteReviewer } = useDeleteClReviewers()
  const handleDeleteReviewer = (user_name: string) => {
    deleteReviewer({
      link: id,
      data: {
        reviewer_usernames: [user_name]
      }
    })
  }

  const [tab, setTab] = useAtom(tabAtom)
  const openedBuildTabRef = useRef(false)

  // Open Checks tab when landing with ?tab=check or ?build= shared link.
  useEffect(() => {
    if (!router.isReady || openedBuildTabRef.current) return

    const build = router.query.build
    const tab = router.query.tab

    if (tab === 'check' || (typeof build === 'string' && build)) {
      setTab('check')
      openedBuildTabRef.current = true
    }
  }, [router.isReady, router.query.build, router.query.tab, setTab])

  const renderStatusPill = () => {
    if (!clDetail?.status) return null

    const normalizedStatus = clDetail.status.toLowerCase()

    let bgClass = ''
    let label: string = clDetail.status as string
    let Icon: React.ElementType | null = null

    switch (normalizedStatus) {
      case 'open':
        bgClass = 'bg-[#1f883d]'
        label = 'Open'
        Icon = GitPullRequestIcon
        break
      case 'draft':
        bgClass = 'bg-[#6e7781]'
        label = 'Draft'
        Icon = GitPullRequestDraftIcon
        break
      case 'merged':
        bgClass = 'bg-purple-500'
        label = 'Merged'
        Icon = GitMergeIcon
        break
      case 'closed':
        bgClass = 'bg-red-600'
        label = 'Closed'
        Icon = GitPullRequestClosedIcon
        break
      default:
        return null
    }

    return (
      <div className='mt-3 flex items-center gap-3'>
        <div
          className={`inline-flex items-center rounded-full px-4 py-2 text-sm leading-none font-medium text-white ${bgClass}`}
        >
          {Icon && <Icon size={16} className='mr-1 text-white' />}
          <span>{label}</span>
        </div>
        {(clDetail as any)?.path && <span className='text-tertiary text-sm'>{(clDetail as any).path}</span>}
      </div>
    )
  }

  return (
    <ThemeProvider colorMode={theme === 'dark' ? 'dark' : 'light'}>
      <BaseStyles>
        <div className='h-screen overflow-auto p-6'>
          {clDetail && (
            <>
              <TitleInput title={clDetail.title} whoami='cl' id={id} callback={() => refetch({ throwOnError: true })} />
              {renderStatusPill()}
            </>
          )}
          <div>
            <TabLayout>
              {tab === 'check' && id && <Checks cl={id} path={(clDetail as any)?.path} prName={clDetail?.title} />}
              {tab === 'conversation' && (
                <ConversationTab
                  detailIsLoading={detailIsLoading}
                  ReviewIsLoading={ReviewIsLoading}
                  clDetail={clDetail}
                  id={id}
                  editorRef={editorRef}
                  reviewers={reviewers}
                  dropzone={dropzone}
                  handleChange={handleChange}
                  onKeyDownScrollHandler={onKeyDownScrollHandler}
                  isReactionPickerOpen={isReactionPickerOpen}
                  setIsReactionPickerOpen={setIsReactionPickerOpen}
                  login={login}
                  clCloseIsPending={clCloseIsPending}
                  handleClClose={handleClClose}
                  closeHint={closeHint}
                  clReopenIsPending={clReopenIsPending}
                  handleClReopen={handleClReopen}
                  clCommentIsPending={clCommentIsPending}
                  send_comment={send_comment}
                  availableAvatars={availableAvatars}
                  review_open={review_open}
                  review_handleOpenChange={review_handleOpenChange}
                  handleReviewers={handleReviewers}
                  review_fetchSelected={review_fetchSelected}
                  memberMap={memberMap}
                  handleDeleteReviewer={handleDeleteReviewer}
                  avatars={avatars}
                  open={open}
                  handleOpenChange={handleOpenChange}
                  handleAssignees={handleAssignees}
                  fetchSelected={fetchSelected}
                  labels={labels}
                  label_open={label_open}
                  label_handleOpenChange={label_handleOpenChange}
                  handleLabels={handleLabels}
                  label_fetchSelected={label_fetchSelected}
                  labelMap={labelMap}
                  buttonClasses={buttonClasses}
                />
              )}
              {tab === 'filechange' && <FileChangeTab id={id} />}
            </TabLayout>
          </div>
        </div>
      </BaseStyles>
    </ThemeProvider>
  )
}

CLDetailPage.getProviders = (page, pageProps) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default CLDetailPage
