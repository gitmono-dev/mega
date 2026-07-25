'use client'

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ActionList, FormControl, SelectPanel, Stack, Text, TextInput, type SelectPanelItemInput } from '@primer/react'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import '@primer/primitives/dist/css/functional/themes/light.css'

import { Button, HelpIcon, Link, PicturePlusIcon } from '@gitmono/ui'

import { EMPTY_HTML } from '@/atoms/markdown'
import { useHandleBottomScrollOffset } from '@/components/NoteEditor/useHandleBottomScrollOffset'
import { ComposerReactionPicker } from '@/components/Reactions/ComposerReactionPicker'
import { SimpleNoteContent, SimpleNoteContentRef } from '@/components/SimpleNoteEditor/SimpleNoteContent'
import { useScope } from '@/contexts/scope'
import { usePostIssueSubmit } from '@/hooks/issues/usePostIssueSubmit'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useGetOrganizationMember } from '@/hooks/useGetOrganizationMember'
import { useUploadHelpers } from '@/hooks/useUploadHelpers'
import { apiErrorToast } from '@/utils/apiErrorToast'
import { trimHtml } from '@/utils/trimHtml'

import { MemberAvatar } from '../MemberAvatar'
import { pickWithReflect } from './utils/pickWithReflectDeep'
import { splitFun, useAvatars, useLabelMap, useLabels, useMemberMap } from './utils/sideEffect'

// import { OrganizationMember } from '@gitmono/types/generated'

export default function IssueNewPage() {
  const { scope } = useScope()
  const [title, setTitle] = useState('')
  const [loadings, setLoadings] = useState<boolean[]>([])
  const router = useRouter()
  const { mutate: submitNewIssue } = usePostIssueSubmit()
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
  // const [avatarTems, setAvatarItems] = useState<SelectPanelItemInput[]>(avatars)

  const labels = useLabels()

  const [isReactionPickerOpen, setIsReactionPickerOpen] = useState(false)
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

  const submit = useCallback(() => {
    const currentContentHTML = editorRef.current?.editor?.getHTML() ?? '<p></p>'

    if (trimHtml(currentContentHTML) === '' || !title) {
      toast.error('please fill the issue list first!')
      return
    }
    set_to_loading(3)
    submitNewIssue(
      { data: { title, description: currentContentHTML } },
      {
        onSuccess: (_response) => {
          editorRef.current?.clearAndBlur()
          cancel_loading(3)
          toast.success('success')
          router.push(`/${router.query.org}/issue`)
        },
        onError: () => apiErrorToast
      }
    )
  }, [router, title, submitNewIssue])

  const editorRef = useRef<SimpleNoteContentRef>(null)
  const onKeyDownScrollHandler = useHandleBottomScrollOffset({
    editor: editorRef.current?.editor
  })
  const { dropzone } = useUploadHelpers({
    upload: editorRef.current?.uploadAndAppendAttachments
  })

  const memberMap = useMemberMap()

  const labelMap = useLabelMap()

  // const createGroup = (selected: SelectPanelItemInput[]) => {
  //   setAvatarItems((prev) => {
  //     return mergeAndDeduplicate(prev, selected, 'end', '1')
  //   })
  // }

  // const groupMetadata: GroupedListProps['groupMetadata'] = [
  //   { groupId: '1', header: { title: 'Group assignees', variant: 'filled' } },
  //   { groupId: '2', header: { title: 'Items', variant: 'filled' } },
  //   { groupId: 'end', header: { title: 'Suggestions', variant: 'filled' } }
  // ]

  return (
    <>
      <div className='container flex h-screen gap-4 overflow-auto p-10'>
        {avatarUser && <MemberAvatar member={avatarUser} />}
        <Stack className='flex-1'>
          <FormControl>
            <FormControl.Label>Create a new issue</FormControl.Label>
          </FormControl>
          <div className='flex items-center gap-2'>
            <Text size='small' weight='light'>
              Blank issue
            </Text>
            <div className='text-[#0969e1]'>
              <Text size='small'>Choose a different template</Text>
            </div>
          </div>

          <div className='flex gap-9'>
            <div className='flex w-[70%] flex-col'>
              <div className='h-[20%]'>
                <FormControl className='w-full' required>
                  <FormControl.Label>Add a title</FormControl.Label>
                  <TextInput
                    placeholder='Title'
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    className='new-issue-input no-border-input w-full'
                  />
                </FormControl>
              </div>
              <FormControl>
                <FormControl.Label>Add a description</FormControl.Label>
                <input {...dropzone.getInputProps()} />
                <div className='h-[550px] w-full rounded-lg border p-6'>
                  <SimpleNoteContent
                    commentId='temp' //  Temporary filling, replacement later
                    ref={editorRef}
                    editable='all'
                    content={EMPTY_HTML}
                    autofocus={true}
                    onKeyDown={onKeyDownScrollHandler}
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
                <div className='flex w-full justify-end gap-4 pt-4'>
                  <Link href={`/${scope}/issue`}>
                    <Button type={'button'}>Cancel</Button>
                  </Link>
                  <Button
                    className='bg-[#1a7b36] text-[#fff]'
                    type={'submit'}
                    loading={loadings[3]}
                    // disabled={!title}
                    onClick={() => submit()}
                  >
                    Submit
                  </Button>
                </div>
              </FormControl>
            </div>
            {/* <SideBar /> */}
            <div className='flex flex-1 flex-col flex-wrap items-center'>
              <BadgeItem
                selectPannelProps={{ title: 'Assign up to 10 people to this issue' }}
                items={avatars}
                title='Assignees'
              >
                {(el) => {
                  const names = Array.from(new Set(splitFun(el)))

                  return (
                    <>
                      {names.map((i, index) => (
                        // eslint-disable-next-line react/no-array-index-key
                        <div key={index} className='mb-4 flex items-center gap-2 px-4 text-sm text-gray-500'>
                          <MemberAvatar size='sm' member={memberMap.get(i)} />
                          <span>{i}</span>
                        </div>
                      ))}
                    </>
                  )
                }}
              </BadgeItem>
              <BadgeItem selectPannelProps={{ title: 'Apply labels to this issue' }} items={labels} title='Labels'>
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
            </div>
          </div>
        </Stack>
      </div>
    </>
  )
}

type SelectPanelExcludedProps =
  'open' | 'onOpenChange' | 'items' | 'selected' | 'onSelectedChange' | 'onFilterChange' | 'renderAnchor' | 'variant'

export const BadgeItem = ({
  title,
  selectPannelProps,
  items,
  children,
  handleGroup,
  open,
  onOpenChange,
  selected,
  emptyExtra
}: {
  title: string
  selectPannelProps?: Omit<React.ComponentProps<typeof SelectPanel>, SelectPanelExcludedProps> & {
    variant?: 'anchored'
  }
  items: SelectPanelItemInput[]
  children?: (el: React.ReactNode) => React.ReactNode
  handleGroup?: (selected: SelectPanelItemInput[]) => void
  open?: boolean
  onOpenChange?: (open: boolean) => void
  selected?: SelectPanelItemInput[]
  emptyExtra?: React.ReactNode
}) => {
  // const { open, onOpenChange } = selectPannelProps ?? {}
  // open有传，则是受控组件，没传就是非受控组件(内部管理)
  const isControl = open !== undefined

  const [control, setControl] = useState(false)

  const currentOpen = isControl ? open : control

  const [chose, setChose] = useState<SelectPanelItemInput[]>([])

  const [filter, setFilter] = React.useState('')

  useEffect(() => {
    selected && selected?.length !== 0 ? setChose([...selected]) : setChose([])
  }, [selected])

  const handleOpenChange = (nextOpen: boolean, _gesture?: any) => {
    // 有onOpenChange就传递
    onOpenChange?.(nextOpen)
    if (!isControl) {
      setControl(nextOpen)
    }
  }

  const filteredItems = items.filter(
    (item) =>
      // design guidelines say to always show selected item in the list
      chose.some((selectedItem) => selectedItem.text === item.text) ||
      // then filter the rest
      item.text?.toLowerCase().startsWith(filter.toLowerCase())
  )

  return (
    <>
      <div className='w-full'>
        <ActionList>
          <SelectPanel
            className='no-border-input new-issue-side-input'
            overlayProps={{ width: 'medium', height: 'medium', overflow: 'auto' }}
            renderAnchor={({ children: container, ...anchorProps }) => {
              return (
                <div {...anchorProps} className='w-full'>
                  <ActionList.Item>
                    <div className='flex justify-between'>
                      {title}
                      <HelpIcon />
                    </div>
                  </ActionList.Item>
                  {container ? (
                    children?.(container)
                  ) : (
                    <SideBarItem emptyState={`No ${title.toLowerCase()}`} extra={emptyExtra} />
                  )}
                </div>
              )
            }}
            open={currentOpen}
            onOpenChange={(open, gesture) => handleOpenChange(open, gesture)}
            items={filteredItems}
            selected={chose}
            onSelectedChange={(selected: SelectPanelItemInput[]) => {
              setChose(selected)
              handleGroup?.(selected)
            }}
            onFilterChange={setFilter}
            {...selectPannelProps}
          ></SelectPanel>
        </ActionList>
      </div>
    </>
  )
}

export const SideBarItem = ({ emptyState, extra }: { emptyState: string; extra?: React.ReactNode }) => {
  return (
    <>
      <div className='mx-4 text-sm text-gray-500'>
        <div className='mt-4 mb-4'>{emptyState}</div>
        {extra}
        <div className='h-[1px] w-full bg-gray-200'></div>
      </div>
    </>
  )
}
