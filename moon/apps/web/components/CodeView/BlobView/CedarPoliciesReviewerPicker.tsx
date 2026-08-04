'use client'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import toast from 'react-hot-toast'

import { LoadingSpinner } from '@gitmono/ui'

import { useGetSyncMembers } from '@/hooks/useGetSyncMembers'

import {
  inferPathPatternFromFilePath,
  reviewersForPathPattern,
  rewriteReviewersForPathPattern
} from './cedarPoliciesUtils'

interface CedarPoliciesReviewerPickerProps {
  filePath: string
  fileContent: string
  onContentGenerated: (content: string) => void
  disabled?: boolean
}

export function CedarPoliciesReviewerPicker({
  filePath,
  fileContent,
  onContentGenerated,
  disabled = false
}: CedarPoliciesReviewerPickerProps) {
  const [memberSearchQuery, setMemberSearchQuery] = useState('')
  const [selectedReviewers, setSelectedReviewers] = useState<string[]>([])
  const initializedRef = useRef(false)

  const pathPattern = useMemo(() => inferPathPatternFromFilePath(filePath), [filePath])

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

  const parsedReviewers = useMemo(() => reviewersForPathPattern(fileContent, pathPattern), [fileContent, pathPattern])

  useEffect(() => {
    if (initializedRef.current) return

    setSelectedReviewers(parsedReviewers)
    initializedRef.current = true
  }, [parsedReviewers])

  const applyReviewers = useCallback(
    (reviewers: string[]) => {
      if (reviewers.length === 0) {
        toast.error('Select at least one reviewer')
        return
      }

      const next = rewriteReviewersForPathPattern(fileContent, pathPattern, reviewers)

      onContentGenerated(next)
    },
    [fileContent, onContentGenerated, pathPattern]
  )

  const handleToggle = useCallback(
    (username: string) => {
      setSelectedReviewers((prev) => {
        const next = prev.includes(username) ? prev.filter((u) => u !== username) : [...prev, username].sort()

        if (next.length === 0) {
          toast.error('Select at least one reviewer')
          return prev
        }

        applyReviewers(next)
        return next
      })
    },
    [applyReviewers]
  )

  const pathLabel = pathPattern === '' ? '(all paths)' : pathPattern

  return (
    <div className='border-b border-[#d0d9e0] bg-[#f9fbfd] px-4 py-3'>
      <div className='mb-2 flex items-center justify-between gap-2'>
        <div>
          <div className='text-sm font-semibold text-gray-900'>Required reviewers</div>
          <div className='text-xs text-gray-500'>
            Select GitHub logins for{' '}
            <code className='rounded bg-gray-100 px-1'>startsWith(&quot;{pathPattern}&quot;)</code> →{' '}
            <span className='text-gray-600'>{pathLabel}</span>
          </div>
        </div>
      </div>

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
        disabled={disabled || !!membersError}
      />

      <div className='max-h-48 overflow-y-auto rounded-md border border-gray-200 bg-white'>
        {isMembersLoading ? (
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
              const isSelected = selectedReviewers.includes(cedarId)

              return (
                <label
                  key={member.user.id}
                  className={`flex cursor-pointer items-center px-3 py-2 transition-colors hover:bg-gray-50 ${
                    isSelected ? 'bg-blue-50' : ''
                  } ${disabled ? 'pointer-events-none opacity-60' : ''}`}
                >
                  <input
                    type='checkbox'
                    checked={isSelected}
                    onChange={() => handleToggle(cedarId)}
                    disabled={disabled}
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
                      reviewer
                    </span>
                  )}
                </label>
              )
            })}
          </div>
        )}
      </div>

      <div className='mt-2 text-xs text-gray-500'>
        Selected: {selectedReviewers.length > 0 ? selectedReviewers.join(', ') : 'none'}
      </div>
    </div>
  )
}
