'use client'

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { IssueClosedIcon, IssueReopenedIcon } from '@primer/octicons-react'
import { Stack } from '@primer/react'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { CommonResultIssueDetailRes } from '@gitmono/types'
import { Button, LoadingSpinner, PicturePlusIcon } from '@gitmono/ui'

import { EMPTY_HTML } from '@/atoms/markdown'
import { useHandleBottomScrollOffset } from '@/components/NoteEditor/useHandleBottomScrollOffset'
import { ComposerReactionPicker } from '@/components/Reactions/ComposerReactionPicker'
import { SimpleNoteContent, SimpleNoteContentRef } from '@/components/SimpleNoteEditor/SimpleNoteContent'
import { useGetIssueDetail } from '@/hooks/issues/useGetIssueDetail'
import { usePostIssueAssignees } from '@/hooks/issues/usePostIssueAssignees'
import { usePostIssueClose } from '@/hooks/issues/usePostIssueClose'
import { usePostIssueComment } from '@/hooks/issues/usePostIssueComment'
import { usePostIssueReopen } from '@/hooks/issues/usePostIssueReopen'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { usePostIssueLabels } from '@/hooks/usePostIssueLabels'
import { useUploadHelpers } from '@/hooks/useUploadHelpers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { trimHtml } from '@/utils/trimHtml'

import { ActorAvatar } from '../ActorAvatar'
import TimelineItems from '../ClView/TimelineItems'
import { MemberAvatar } from '../MemberAvatar'
import { BadgeItem } from './IssueNewPage'
import TitleInput from './TitleInput'
import { pickWithReflect } from './utils/pickWithReflectDeep'
import {
  splitFun,
  useAssigneesSelector,
  useAvatars,
  useChange,
  useLabelMap,
  useLabels,
  useLabelsSelector,
  useMemberMap
} from './utils/sideEffect'
import { editIdAtom, FALSE_EDIT_VAL, issueIdAtom, refreshAtom } from './utils/store'
import { WorkWithChatDialog } from './WorkWithChatDialog'

export default function IssueDetailPage({ link }: { link: string }) {
  const [id] = useAtom(issueIdAtom)
  const [login, setLogin] = useState(false)
  const [info, setInfo] = useState<Partial<CommonResultIssueDetailRes['data']>>({
    status: '',
    conversations: [],
    title: '',
    assignees: [],
    labels: []
  })
  const [buttonLoading, setButtonLoading] = useState<{ [key: string]: boolean }>({
    comment: false,
    close: false,
    reopen: false
  })
  const [isReactionPickerOpen, setIsReactionPickerOpen] = useState(false)
  const setLoading = (key: string, value: boolean) => {
    setButtonLoading((prev) => ({ ...prev, [key]: value }))
  }

  const { closeHint, needComment, handleChange, handleCloseChange } = useChange({})
  // const [closeHint, setCloseHint] = useState('Close issue')

  const { mutate: closeIssue } = usePostIssueClose()

  const { mutate: reopenIssue } = usePostIssueReopen()

  const { mutate: saveComment } = usePostIssueComment()

  const { mutate: issueAssignees } = usePostIssueAssignees()

  const { mutate: issueLabels } = usePostIssueLabels()

  const { data: issueDetailObj, error, isError, refetch, isLoading: detailIsLoading } = useGetIssueDetail(link)

  const issueDetail = issueDetailObj?.data as CommonResultIssueDetailRes['data'] | undefined

  const applyDetailData = (detail: CommonResultIssueDetailRes | undefined) => {
    if (!detail || !detail.req_result || !detail.data) return
    setInfo({
      title: detail.data.title,
      status: detail.data.status,
      conversations: detail.data.conversations,
      assignees: detail.data.assignees,
      labels: detail.data.labels
    })

    // selectRef.current = detail.data.assignees
  }

  const fetchDetail = useCallback(() => {
    if (error || isError) return
    applyDetailData(issueDetailObj)
    setLogin(true)
  }, [issueDetailObj, error, isError])

  useEffect(() => {
    fetchDetail()
  }, [fetchDetail, link])

  const [refresh, setRefresh] = useAtom(refreshAtom)
  const [_, setEditId] = useAtom(editIdAtom)

  useEffect(() => {
    const load = async () => {
      await refetch()
      setEditId(FALSE_EDIT_VAL)
      setRefresh(0)
    }

    load()
  }, [refresh, refetch, setEditId, setRefresh])

  const [_loadings, setLoadings] = useState<boolean[]>([])
  const router = useRouter()

  const set_to_loading = (index: number) => {
    setLoadings((prevLoadings) => {
      const newLoadings = [...prevLoadings]

      newLoadings[index] = true
      return newLoadings
    })
  }

  const cancel_loading = (index: number) => {
    setLoadings((prevLoadings) => {
      const newLoadings = [...prevLoadings]

      newLoadings[index] = false
      return newLoadings
    })
  }

  const save_comment = useCallback(() => {
    const currentContentHTML = editorRef.current?.editor?.getHTML() ?? '<p></p>'

    if (trimHtml(currentContentHTML) === '') {
      toast.error('comment can not be empty!')
      return
    }
    setLoading('comment', true)
    set_to_loading(3)
    saveComment(
      { link, data: { content: currentContentHTML } },
      {
        onSuccess: async () => {
          editorRef.current?.clearAndBlur()
          const { data: issueDetailObj } = await refetch({ throwOnError: true })

          applyDetailData(issueDetailObj)
          cancel_loading(3)
          toast.success('comment successfully!')
        },
        onError: apiErrorToast,
        onSettled: () => setLoading('comment', false)
      }
    )
  }, [link, refetch, saveComment])

  const close_issue = useCallback(() => {
    if (closeHint === 'Close with comment') {
      save_comment()
    }

    setLoading('close', true)
    set_to_loading(3)
    closeIssue(
      { link },
      {
        onSuccess: () => {
          router.push(`/${router.query.org}/issue`)
          cancel_loading(3)
        },
        onError: apiErrorToast,
        onSettled: () => setLoading('close', false)
      }
    )
  }, [link, router, closeIssue, closeHint, save_comment])

  const reopen_issue = useCallback(() => {
    setLoading('reopen', true)
    set_to_loading(3)
    if (needComment.current) {
      save_comment()
      needComment.current = false
    }
    reopenIssue(
      { link },
      {
        onSuccess: () => {
          router.push(`/${router.query.org}/issue`)
        },
        onError: apiErrorToast,
        onSettled: () => setLoading('reopen', false)
      }
    )
  }, [link, router, reopenIssue, save_comment, needComment])

  const editorRef = useRef<SimpleNoteContentRef>(null)
  const onKeyDownScrollHandler = useHandleBottomScrollOffset({
    editor: editorRef.current?.editor
  })
  const { dropzone } = useUploadHelpers({
    upload: editorRef.current?.uploadAndAppendAttachments
  })

  const { data } = useGetCurrentUser()
  const { data: user } = useGetOrganizationMember({ username: data?.username })

  const avatarUser = useMemo(() => {
    if (!user) return undefined
    return {
      deactivated: user['deactivated'],
      user: pickWithReflect(user?.user ?? {}, [
        'id',
        'display_name',
        'username',
        'avatar_urls',
        'notifications_paused',
        'integration'
      ])
    }
  }, [user])

  const avatars = useAvatars()

  const labels = useLabels()

  const memberMap = useMemberMap()

  const labelMap = useLabelMap()

  const { open, handleAssignees, handleOpenChange, fetchSelected } = useAssigneesSelector({
    assignees: info?.assignees ?? [],
    assignRequest: (selected) =>
      issueAssignees(
        {
          data: {
            assignees: selected,
            item_id: Number(id),
            link
          }
        },
        {
          onSuccess: async () => {
            editorRef.current?.clearAndBlur()
            const { data: issueDetailObj } = await refetch({ throwOnError: true })

            applyDetailData(issueDetailObj)
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
    labels: info?.labels ?? [],
    updateLabelsRequest: (selected) => {
      issueLabels(
        {
          data: {
            item_id: Number(id),
            label_ids: selected,
            link
          }
        },
        {
          onSuccess: async () => {
            editorRef.current?.clearAndBlur()
            const { data: issueDetailObj } = await refetch({ throwOnError: true })

            applyDetailData(issueDetailObj)
          },
          onError: apiErrorToast
        }
      )
    }
  })

  return (
    <>
      <div className='h-screen overflow-auto pt-10'>
        {info?.title && (
          <div className='px-10 pb-4 text-xl'>
            {issueDetail && (
              <TitleInput
                title={issueDetail.title}
                id={link}
                whoami='issue'
                callback={() => refetch({ throwOnError: true })}
              />
            )}
          </div>
        )}
        <div className='container flex gap-4 p-10 pt-0'>
          {avatarUser && <MemberAvatar member={avatarUser} />}

          <Stack className='flex-1'>
            <div className='flex h-[100vh] gap-20'>
              <div className='flex w-[70%] flex-col'>
                {detailIsLoading ? (
                  <div className='flex items-center justify-center'>
                    <LoadingSpinner />
                  </div>
                ) : (
                  issueDetail && <TimelineItems detail={issueDetail} id={link} type='issue' editorRef={editorRef} />
                )}

                {info && info.status === 'open' && (
                  <>
                    <div className='prose mt-4 flex w-full flex-col'>
                      <h2>Add a comment</h2>
                      <input {...dropzone.getInputProps()} />
                      <div className='relative rounded-lg border p-6 pb-12'>
                        <SimpleNoteContent
                          commentId='temp' //  Temporary filling, replacement later
                          ref={editorRef}
                          editable='all'
                          content={EMPTY_HTML}
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
                    <div
                      style={{
                        marginTop: `16px`
                      }}
                      className='mt-4 flex justify-end gap-4'
                    >
                      <Button
                        className='!mb-32'
                        // loading={motivation === 'close' ? loadings[3] : undefined}
                        loading={buttonLoading.close}
                        disabled={!login}
                        onClick={() => close_issue()}
                      >
                        <span className='flex items-center gap-2'>
                          <IssueClosedIcon className='text-[#8250df]' />
                          {closeHint}
                        </span>
                      </Button>
                      <Button
                        className='bg-[#1f883d] text-white'
                        loading={buttonLoading.comment}
                        disabled={!login}
                        onClick={() => save_comment()}
                      >
                        Comment
                      </Button>
                    </div>
                  </>
                )}

                {info && info.status === 'closed' && (
                  <>
                    <div className='prose mt-4 flex w-full flex-col'>
                      <h2>Add a comment</h2>
                      <input {...dropzone.getInputProps()} />
                      <div className='relative rounded-lg border p-6 pb-12'>
                        <SimpleNoteContent
                          commentId='temp' //  Temporary filling, replacement later
                          ref={editorRef}
                          editable='all'
                          content={EMPTY_HTML}
                          autofocus={true}
                          onKeyDown={onKeyDownScrollHandler}
                          onChange={(html) => handleCloseChange(html)}
                        />
                        <div className='absolute'>
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
                    </div>
                    <div
                      style={{
                        marginTop: `16px`
                      }}
                      className='mt-4 flex justify-end gap-4'
                    >
                      <Button
                        className='!mb-32'
                        loading={buttonLoading.reopen}
                        disabled={!login}
                        onClick={() => reopen_issue()}
                      >
                        <span className='flex items-center gap-2'>
                          <IssueReopenedIcon className='text-[#1f883d]' />
                          Reopen issue
                        </span>
                      </Button>
                      <Button
                        className='bg-[#1f883d] text-white'
                        loading={buttonLoading.comment}
                        disabled={!login}
                        onClick={() => save_comment()}
                      >
                        Comment
                      </Button>
                    </div>
                  </>
                )}
              </div>
              {/* <SideBar /> */}
              <div className='flex flex-1 flex-col flex-nowrap items-center'>
                <BadgeItem
                  selectPannelProps={{ title: 'Assign up to 10 people to this issue' }}
                  items={avatars}
                  title='Assignees'
                  handleGroup={(selected) => handleAssignees(selected)}
                  open={open}
                  onOpenChange={(open) => handleOpenChange(open)}
                  selected={fetchSelected}
                >
                  {(el) => {
                    const names = Array.from(new Set(splitFun(el)))

                    return (
                      <>
                        {names.map((i, index) => {
                          const member = memberMap.get(i)
                          const displayName = member?.user?.github_login?.trim() || member?.user?.username || i

                          return (
                            // eslint-disable-next-line react/no-array-index-key
                            <div key={index} className='mb-4 flex items-center gap-2 px-4 text-sm text-gray-500'>
                              <ActorAvatar size='sm' member={member} username={displayName} />
                              <span>{displayName}</span>
                            </div>
                          )
                        })}
                      </>
                    )
                  }}
                </BadgeItem>
                <BadgeItem
                  selectPannelProps={{ title: 'Apply labels to this issue' }}
                  items={labels}
                  title='Labels'
                  handleGroup={(selected) => handleLabels(selected)}
                  open={label_open}
                  onOpenChange={(open) => label_handleOpenChange(open)}
                  selected={label_fetchSelected}
                >
                  {(el) => {
                    const names = splitFun(el)

                    return (
                      <>
                        <div className='flex flex-wrap items-start px-4'>
                          {names.map((i, index) => {
                            const label = labelMap.get(i) ?? {}

                            return (
                              // eslint-disable-next-line react/no-array-index-key
                              <div key={index} className='mb-4 flex items-center justify-center pr-2'>
                                <div
                                  className='rounded-full border px-2 text-sm text-[#fff]'

                                  style={{ backgroundColor: label.color, borderColor: label.color }}
                                >
                                  {label.name}
                                </div>
                              </div>
                            )
                          })}
                        </div>
                      </>
                    )
                  }}
                </BadgeItem>
                <BadgeItem title='Type' items={labels} />
                <BadgeItem title='Projects' items={labels} />
                <BadgeItem title='Milestones' items={labels} />

                {/* Work with Chat Dialog */}
                <div className='mt-4 w-full'>
                  <WorkWithChatDialog />
                </div>
              </div>
            </div>
          </Stack>
        </div>
      </div>
    </>
  )
}
