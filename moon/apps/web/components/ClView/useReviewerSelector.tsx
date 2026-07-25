import { useMemo, useRef, useState } from 'react'
import type { SelectPanelItemInput as ItemInput } from '@primer/react'

import { ReviewerInfo } from '@gitmono/types'

import { useAvatars } from '@/components/Issues/utils/sideEffect'

type MegaAvatar = ReturnType<typeof useAvatars>[number] & { username?: string }

function avatarApiIdentity(item: ItemInput): string | undefined {
  const mega = item as MegaAvatar
  // Prefer Campsite username for API identity; SelectPanel text may be github_login.
  if (typeof mega.username === 'string' && mega.username) return mega.username
  if (typeof item.text === 'string' && item.text) return item.text
  return undefined
}

function avatarMatchesHandle(user: MegaAvatar, handle: string) {
  return user.text === handle || user.username === handle
}

export const useReviewerSelector = ({
  reviewers,
  reviewRequest,
  avatars
}: {
  reviewers: ReviewerInfo[]
  reviewRequest: (selected: string[]) => void
  avatars: ReturnType<typeof useAvatars>
}) => {
  const initialReviewers = useMemo(() => reviewers.map((item) => item.username), [reviewers])
  const [selectedUsers, setSelectedUsers] = useState<string[]>([])
  const shouldFetch = useRef(false)
  const [open, setOpen] = useState(false)

  const handleAssignees = (selected: ItemInput[]) => {
    const newSelection = [
      ...selected.map((i) => avatarApiIdentity(i)).filter((t): t is string => typeof t === 'string')
    ]

    setSelectedUsers(newSelection)
    shouldFetch.current = true
  }

  const handleOpenChange = (open: boolean) => {
    setOpen(open)
    if (!open && shouldFetch.current) {
      const newlySelected = selectedUsers.filter((user) => !initialReviewers.some((existing) => existing === user))

      if (newlySelected.length > 0) {
        reviewRequest(newlySelected)
      }
      shouldFetch.current = false
    }
  }

  const fetchSelected = useMemo(() => {
    return avatars.filter((user) => initialReviewers.some((handle) => avatarMatchesHandle(user as MegaAvatar, handle)))
  }, [avatars, initialReviewers])

  const availableAvatars = useMemo(() => {
    return avatars.filter((user) => !initialReviewers.some((handle) => avatarMatchesHandle(user as MegaAvatar, handle)))
  }, [avatars, initialReviewers])

  return {
    open,
    handleAssignees,
    handleOpenChange,
    fetchSelected,
    availableAvatars
  }
}
