'use client'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import toast from 'react-hot-toast'

import { LoadingSpinner } from '@gitmono/ui'

import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useAdminList } from '@/hooks/admin/useAdminList'
import { useGenerateMegaCedar } from '@/hooks/admin/useGenerateMegaCedar'
import { useGetSyncMembers } from '@/hooks/useGetSyncMembers'

const ADMIN_GROUP = 'UserGroup::"admin"'

function parseAdminsFromCedarContent(content: string): string[] {
  try {
    const parsed = JSON.parse(content) as {
      users?: Record<string, { parents?: string[] }>
    }
    const users = parsed.users || {}
    const admins: string[] = []

    for (const [key, value] of Object.entries(users)) {
      const parents = value?.parents || []

      if (!parents.includes(ADMIN_GROUP)) continue

      const match = key.match(/^User::"(.+)"$/)

      if (match?.[1]) {
        admins.push(match[1])
      }
    }

    return admins.sort()
  } catch {
    return []
  }
}

interface MegaCedarAdminPickerProps {
  fileContent: string
  onContentGenerated: (content: string) => void
  disabled?: boolean
}

export function MegaCedarAdminPicker({ fileContent, onContentGenerated, disabled = false }: MegaCedarAdminPickerProps) {
  const [memberSearchQuery, setMemberSearchQuery] = useState('')
  const [selectedAdmins, setSelectedAdmins] = useState<string[]>([])
  const initializedRef = useRef(false)

  const {
    members,
    isLoading: isMembersLoading,
    refetch: refetchMembers,
    error: membersError
  } = useGetSyncMembers({
    query: memberSearchQuery,
    excludeCurrentUser: false,
    enabled: true
  })

  const { data: adminCheck, isLoading: isAdminCheckLoading } = useAdminCheck()
  const isMonoAdmin = adminCheck?.data?.is_admin === true
  const canUseAdminApis = !isAdminCheckLoading && isMonoAdmin

  const {
    data: adminListData,
    isLoading: isAdminListLoading,
    isError: isAdminListError,
    error: adminListError
  } = useAdminList({ enabled: canUseAdminApis })

  const generateCedar = useGenerateMegaCedar()

  const parsedAdmins = useMemo(() => parseAdminsFromCedarContent(fileContent), [fileContent])

  useEffect(() => {
    if (initializedRef.current) return

    if (parsedAdmins.length > 0) {
      setSelectedAdmins(parsedAdmins)
      initializedRef.current = true
      return
    }

    if (isAdminCheckLoading) return

    if (!canUseAdminApis) {
      initializedRef.current = true
      return
    }

    if (isAdminListLoading) return

    if (adminListData?.data?.admins) {
      setSelectedAdmins([...(adminListData.data.admins || [])].sort())
    }

    initializedRef.current = true
  }, [parsedAdmins, adminListData, isAdminListLoading, canUseAdminApis, isAdminCheckLoading])

  const regenerateContent = useCallback(
    async (admins: string[]) => {
      if (admins.length === 0) {
        toast.error('Select at least one admin')
        return
      }

      if (!isMonoAdmin) {
        toast.error('Admin access required to regenerate .mega_cedar.json')
        return
      }

      try {
        const response = await generateCedar.mutateAsync({ admins })
        const content = response?.data?.content

        if (content) {
          onContentGenerated(content)
        } else {
          toast.error('Failed to generate .mega_cedar.json')
        }
      } catch {
        // apiErrorToast handles the error
      }
    },
    [generateCedar, onContentGenerated, isMonoAdmin]
  )

  const handleToggle = useCallback(
    (username: string) => {
      if (!isMonoAdmin) {
        toast.error('Admin access required to regenerate .mega_cedar.json')
        return
      }

      setSelectedAdmins((prev) => {
        const next = prev.includes(username) ? prev.filter((u) => u !== username) : [...prev, username].sort()

        if (next.length === 0) {
          toast.error('Select at least one admin')
          return prev
        }

        void regenerateContent(next)
        return next
      })
    },
    [regenerateContent, isMonoAdmin]
  )

  const isLoading = isMembersLoading || isAdminCheckLoading || (canUseAdminApis && isAdminListLoading)
  const adminForbidden =
    (!isAdminCheckLoading && !isMonoAdmin) ||
    (isAdminListError &&
      adminListError instanceof Error &&
      (adminListError.name === 'ForbiddenError' || /admin access required/i.test(adminListError.message)))

  return (
    <div className='border-b border-[#d0d9e0] bg-[#f9fbfd] px-4 py-3'>
      <div className='mb-2 flex items-center justify-between gap-2'>
        <div>
          <div className='text-sm font-semibold text-gray-900'>System admins</div>
          <div className='text-xs text-gray-500'>
            Select GitHub logins to regenerate <code className='rounded bg-gray-100 px-1'>.mega_cedar.json</code>
          </div>
        </div>
        {generateCedar.isPending && (
          <div className='flex items-center gap-2 text-xs text-gray-500'>
            <LoadingSpinner />
            Generating…
          </div>
        )}
      </div>

      {adminForbidden && (
        <div className='mb-3 rounded-md border border-amber-200 bg-amber-50 p-3'>
          <p className='text-sm font-medium text-amber-900'>Admin access required</p>
          <p className='mt-1 text-xs text-amber-800'>
            You can view the current admin list from the file, but regenerating{' '}
            <code className='rounded bg-amber-100 px-1'>.mega_cedar.json</code> requires mono admin permission.
          </p>
        </div>
      )}

      {membersError && (
        <div className='mb-3 rounded-md border border-red-200 bg-red-50 p-3'>
          <p className='mb-1 text-sm font-medium text-red-800'>Failed to load organization members</p>
          <button
            type='button'
            onClick={() => refetchMembers()}
            className='text-sm font-medium text-red-600 underline hover:text-red-800'
          >
            Try again
          </button>
        </div>
      )}

      <input
        type='text'
        value={memberSearchQuery}
        onChange={(e) => setMemberSearchQuery(e.target.value)}
        className='mb-3 w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-hidden'
        placeholder='Search members by name or username...'
        disabled={disabled || !!membersError || generateCedar.isPending || adminForbidden}
      />

      <div className='max-h-48 overflow-y-auto rounded-md border border-gray-200 bg-white'>
        {isLoading ? (
          <div className='flex items-center justify-center gap-2 py-8 text-sm text-gray-500'>
            <LoadingSpinner />
            Loading members…
          </div>
        ) : members.length === 0 ? (
          <div className='py-8 text-center text-sm text-gray-500'>No members found</div>
        ) : (
          <div className='divide-y divide-gray-100'>
            {members.map((member) => {
              const cedarId = member.user.github_login || member.user.username
              const isSelected = selectedAdmins.includes(cedarId)

              return (
                <label
                  key={member.user.id}
                  className={`flex cursor-pointer items-center px-3 py-2 transition-colors hover:bg-gray-50 ${
                    isSelected ? 'bg-blue-50' : ''
                  } ${disabled || generateCedar.isPending || adminForbidden ? 'pointer-events-none opacity-60' : ''}`}
                >
                  <input
                    type='checkbox'
                    checked={isSelected}
                    onChange={() => handleToggle(cedarId)}
                    disabled={disabled || generateCedar.isPending || adminForbidden}
                    className='mr-3 h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500'
                  />
                  <img
                    src={member.user.avatar_urls?.sm || ''}
                    alt={member.user.display_name}
                    className='mr-3 h-7 w-7 shrink-0 rounded-full border border-gray-200'
                  />
                  <div className='min-w-0 flex-1'>
                    <p className='truncate text-sm font-medium text-gray-900'>{member.user.display_name}</p>
                    <p className='truncate text-xs text-gray-500'>
                      @{cedarId}
                      {member.user.github_login ? ' (GitHub)' : ''}
                    </p>
                  </div>
                  {isSelected && (
                    <span className='ml-2 rounded-full bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-700'>
                      admin
                    </span>
                  )}
                </label>
              )
            })}
          </div>
        )}
      </div>

      <div className='mt-2 text-xs text-gray-500'>
        Selected: {selectedAdmins.length > 0 ? selectedAdmins.join(', ') : 'none'}
      </div>
    </div>
  )
}
