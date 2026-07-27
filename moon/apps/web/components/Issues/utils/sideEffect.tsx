import { useMemo, useRef, useState } from 'react'
import type { SelectPanelItemInput as ItemInput } from '@primer/react'
import { useAtom } from 'jotai'

import { LabelItem } from '@gitmono/types'

import { MemberAvatar } from '@/components/MemberAvatar'
import { useScope } from '@/contexts/scope'
import { useSyncedMembers } from '@/hooks/useSyncedMembers'
import { atomWithWebStorage } from '@/utils/atomWithWebStorage'
import { megaUserHandle } from '@/utils/megaUser'

import { extractTextArray } from './extractText'

export const useAvatars = ({ preferGithubLogin = false }: { preferGithubLogin?: boolean } = {}) => {
  const { members } = useSyncedMembers()

  return useMemo(
    () =>
      members?.map((i) => ({
        groupId: 'end',
        text: preferGithubLogin ? megaUserHandle(i.user) : i.user.username,
        // Campsite username kept for matching legacy CL/reviewer records.
        username: i.user.username,
        leadingVisual: () => <MemberAvatar size='sm' member={i} />
      })) || [],
    [members, preferGithubLogin]
  )
}

export const splitFun = (el: React.ReactNode): string[] => {
  return extractTextArray(el)
    .flatMap((name) => name.split(',').map((n) => n.trim()))
    .filter((n) => n.length > 0)
}

export const useMemberMap = () => {
  const { members } = useSyncedMembers()

  return useMemo(() => {
    const map = new Map()

    members?.forEach((i) => {
      map.set(i.user.username, i)
      const github = i.user.github_login?.trim()

      if (github) {
        map.set(github, i)
      }
    })
    return map
  }, [members])
}

export const useLabels = () => {
  const { scope } = useScope()
  const labelsAtom = useMemo(() => atomWithWebStorage<LabelItem[]>(`${scope}:label`, []), [scope])
  const [labels] = useAtom(labelsAtom)

  return useMemo(
    () =>
      labels.map((i) => ({
        text: `${i.name}`,
        id: i.id,
        leadingVisual: () => (
          <div
            className='h-[14px] w-[14px] rounded-full border'

            style={{ backgroundColor: i.color, borderColor: i.color }}
          />
        )
      })),
    [labels]
  )
}

export const useLabelMap = () => {
  const { scope } = useScope()
  const labelsAtom = useMemo(() => atomWithWebStorage<LabelItem[]>(`${scope}:label`, []), [scope])
  const [labels] = useAtom(labelsAtom)

  return useMemo(() => {
    const map = new Map()

    labels.forEach((i) => {
      map.set(i.name, i)
    })
    return map
  }, [labels])
}

// assignees逻辑

type MegaAvatarItem = ReturnType<typeof useAvatars>[number] & { username?: string }

function avatarApiIdentity(item: ItemInput): string | undefined {
  const mega = item as MegaAvatarItem

  if (typeof mega.username === 'string' && mega.username) return mega.username
  if (typeof item.text === 'string' && item.text) return item.text
  return undefined
}

function avatarMatchesStoredHandle(avatar: MegaAvatarItem, handle: string) {
  return avatar.text === handle || avatar.username === handle
}

export const useAssigneesSelector = ({
  assignees,
  assignRequest,
  avatars
}: {
  assignees: string[]
  assignRequest: (selected: string[]) => void
  avatars: ReturnType<typeof useAvatars>
}) => {
  const selectRef = useRef<string[]>([])
  let selects: string[] = assignees as string[]
  const shouldFetch = useRef(false)
  const [open, setOpen] = useState(false)

  const handleAssignees = (selected: ItemInput[]) => {
    selects = [...selected.map((i) => avatarApiIdentity(i)).filter((t): t is string => typeof t === 'string')]
  }

  const handleOpenChange = (open: boolean) => {
    if (selectRef.current.length !== selects.length) {
      shouldFetch.current = true
    } else {
      const set = new Set(selects)

      for (let i = 0; i < selectRef.current.length; i++) {
        if (!set.has(selectRef.current[i])) {
          shouldFetch.current = true
          break
        }
      }
    }

    setOpen(open)
    if (!open && shouldFetch.current) {
      selectRef.current = selects
      assignRequest(selectRef.current)
      setTimeout(() => (shouldFetch.current = false), 0)
    }
  }

  const fetchSelected = useMemo(() => {
    const set = new Set(selectRef.current.length ? selectRef.current : assignees)

    return avatars.filter((user) =>
      [...set].some((handle) => avatarMatchesStoredHandle(user as MegaAvatarItem, handle))
    )
  }, [selectRef, avatars, assignees])

  return {
    open,
    handleAssignees,
    handleOpenChange,
    fetchSelected
  }
}

export const useChange = ({ title = 'Close issue' }: { title?: string }) => {
  const [closeHint, setCloseHint] = useState(title)

  const needComment = useRef(false)
  const handleChange = (html: string) => {
    if (html && html === '<p></p>') {
      setCloseHint(title)
    } else {
      setCloseHint('Close with comment')
    }
  }

  const handleCloseChange = (html: string) => {
    if (html && html === '<p></p>') {
      needComment.current = false
    } else {
      needComment.current = true
    }
  }

  return {
    closeHint,
    needComment,
    handleChange,
    handleCloseChange
  }
}

export const useLabelsSelector = ({
  labels,
  updateLabelsRequest,
  labelList
}: {
  labels: LabelItem[]
  updateLabelsRequest: (selected_id: number[]) => void
  labelList: ReturnType<typeof useLabels>
}) => {
  const selectRef = useRef<number[]>([])
  let selects: number[] = labels.map((i) => i.id)
  const shouldFetch = useRef(false)
  const [open, setOpen] = useState(false)

  const handleLabels = (selected: ItemInput[]) => {
    selects = [...selected.map((i) => i.id).filter((t): t is number => typeof t === 'number')]
  }

  const handleOpenChange = (open: boolean) => {
    if (selectRef.current.length !== selects.length) {
      shouldFetch.current = true
    } else {
      const set = new Set(selects)

      for (let i = 0; i < selectRef.current.length; i++) {
        if (!set.has(selectRef.current[i])) {
          shouldFetch.current = true
          break
        }
      }
    }

    setOpen(open)
    if (!open && shouldFetch.current) {
      selectRef.current = selects
      updateLabelsRequest(selectRef.current)
      setTimeout(() => (shouldFetch.current = false), 0)
    }
  }

  const fetchSelected = useMemo(() => {
    const set = new Set(selectRef.current.length ? selectRef.current : selects)

    return labelList.filter((label) => set.has(label.id))
  }, [selectRef, labelList, selects])

  return {
    open,
    handleLabels,
    handleOpenChange,
    fetchSelected
  }
}
