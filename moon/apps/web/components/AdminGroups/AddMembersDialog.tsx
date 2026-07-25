import React, { useEffect, useState } from 'react'

import { Button, LoadingSpinner } from '@gitmono/ui'

import { useAddAdminGroupMembers } from '@/hooks/admin/useAddAdminGroupMembers'
import { useAdminGroupMembersList } from '@/hooks/admin/useAdminGroupMembersList'
import { useGetSyncMembers } from '@/hooks/useGetSyncMembers'

interface AddMembersDialogProps {
  groupId: number | null
  onClose: () => void
}

export const AddMembersDialog = ({ groupId, onClose }: AddMembersDialogProps) => {
  const [selectedMembers, setSelectedMembers] = useState<string[]>([])
  const [memberSearchQuery, setMemberSearchQuery] = useState('')

  // Get all users
  const {
    members,
    isLoading: isMembersLoading,
    refetch: refetchMembers,
    error: membersError
  } = useGetSyncMembers({
    query: memberSearchQuery,
    excludeCurrentUser: false,
    enabled: groupId !== null
  })

  // Get current group's existing users
  const { data: groupMembersData, isLoading: isGroupMembersLoading } = useAdminGroupMembersList(groupId || 0, {
    pagination: { page: 1, per_page: 1000 }, // Get all group members
    additional: {}
  })

  const addMembersMutation = useAddAdminGroupMembers()

  // Get list of existing member usernames in current group
  const existingMemberUsernames = new Set(groupMembersData?.data?.items?.map((member) => member.username) || [])

  // Fetch members when dialog opens
  useEffect(() => {
    if (groupId !== null) {
      setTimeout(() => {
        refetchMembers()
      }, 100)
    }
  }, [groupId, refetchMembers])

  const handleMemberToggle = (username: string) => {
    setSelectedMembers((prev) => (prev.includes(username) ? prev.filter((u) => u !== username) : [...prev, username]))
  }

  const handleAddMembersSubmit = async () => {
    if (groupId === null || selectedMembers.length === 0) return

    try {
      await addMembersMutation.mutateAsync({
        groupId: groupId,
        usernames: selectedMembers
      })
      // Close dialog and reset state
      handleClose()
    } catch (error) {
      // Error already handled by apiErrorToast
    }
  }

  const handleClose = () => {
    setSelectedMembers([])
    setMemberSearchQuery('')
    onClose()
  }

  if (groupId === null) return null

  return (
    <div className='bg-opacity-50 fixed inset-0 z-50 flex items-center justify-center bg-black p-4'>
      <div className='bg-primary border-primary flex max-h-[90vh] w-full max-w-2xl flex-col rounded-lg border shadow-xl'>
        {/* Fixed header */}
        <div className='shrink-0 border-b border-gray-200 px-6 py-4 dark:border-gray-700'>
          <h2 className='text-primary text-xl font-bold'>Add Members to Group</h2>

          {/* Debug info */}
          <div className='mt-2 text-xs text-gray-500'>
            Debug: All members: {members.length}, Group members: {groupMembersData?.data?.items?.length || 0}, Loading:{' '}
            {isMembersLoading ? 'Yes' : 'No'}
          </div>
        </div>

        {/* Scrollable content area */}
        <div className='flex-1 overflow-y-auto px-6 py-4'>
          {/* Error handling */}
          {membersError && (
            <div className='mb-4 rounded-md border border-red-200 bg-red-50 p-4 dark:border-red-800 dark:bg-red-900/20'>
              <p className='mb-2 text-sm font-medium text-red-800 dark:text-red-200'>
                Failed to load organization members
              </p>
              <p className='mb-3 text-xs text-red-600 dark:text-red-300'>
                {membersError?.message || 'You may not have permission to view organization members.'}
              </p>
              <button
                onClick={() => refetchMembers()}
                className='text-sm font-medium text-red-600 underline hover:text-red-800'
              >
                Try again
              </button>
            </div>
          )}

          {/* Search box */}
          <div className='mb-4'>
            <input
              type='text'
              value={memberSearchQuery}
              onChange={(e) => setMemberSearchQuery(e.target.value)}
              className='border-primary bg-secondary text-primary w-full rounded-md border px-3 py-2 focus:ring-2 focus:ring-blue-500 focus:outline-hidden'
              placeholder='Search members by name or username...'
              disabled={!!membersError}
            />
          </div>

          {/* Members list */}
          <div className='mb-4'>
            {membersError ? (
              <div className='py-8 text-center'>
                <p className='text-tertiary'>Cannot load members due to permission error</p>
              </div>
            ) : isMembersLoading || isGroupMembersLoading ? (
              <div className='flex items-center justify-center py-12'>
                <LoadingSpinner />
                <span className='text-tertiary ml-2'>Loading members...</span>
              </div>
            ) : members.length === 0 ? (
              <div className='py-12 text-center'>
                <p className='text-tertiary mb-3'>No members found</p>
                <button
                  onClick={() => refetchMembers()}
                  className='text-sm font-medium text-blue-600 underline hover:text-blue-800'
                >
                  Retry loading members
                </button>
              </div>
            ) : (
              <div className='space-y-4'>
                {/* Available members to add */}
                {(() => {
                  const availableMembers = members.filter(
                    (member) => !existingMemberUsernames.has(member.user.username)
                  )

                  return availableMembers.length > 0 ? (
                    <div>
                      <h3 className='text-primary mb-2 text-sm font-medium'>
                        Available Members ({availableMembers.length})
                      </h3>
                      <div className='max-h-60 overflow-y-auto rounded-md border border-gray-200 bg-gray-50 dark:border-gray-700 dark:bg-gray-900/50'>
                        <div className='divide-y divide-gray-200 dark:divide-gray-700'>
                          {availableMembers.map((member) => {
                            const isSelected = selectedMembers.includes(member.user.username)

                            return (
                              <div
                                key={member.user.id}
                                className={`flex cursor-pointer items-center px-4 py-3 transition-colors hover:bg-gray-100 dark:hover:bg-gray-800 ${
                                  isSelected ? 'border-l-4 border-blue-500 bg-blue-50 dark:bg-blue-900/20' : ''
                                }`}
                                onClick={() => handleMemberToggle(member.user.username)}
                              >
                                <input
                                  type='checkbox'
                                  checked={isSelected}
                                  onChange={(e) => {
                                    e.stopPropagation()
                                    handleMemberToggle(member.user.username)
                                  }}
                                  onClick={(e) => e.stopPropagation()}
                                  className='mr-3 h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500'
                                />
                                <img
                                  src={member.user.avatar_urls?.sm || ''}
                                  alt={member.user.display_name}
                                  className='mr-3 h-8 w-8 shrink-0 rounded-full border border-gray-200 dark:border-gray-600'
                                />
                                <div className='min-w-0 flex-1'>
                                  <p className='text-primary truncate text-sm font-medium'>
                                    {member.user.display_name}
                                  </p>
                                  <p className='text-tertiary truncate text-xs'>@{member.user.username}</p>
                                </div>
                                <div className='ml-3 flex shrink-0 items-center gap-2'>
                                  <span
                                    className={`rounded-full px-2 py-1 text-xs font-medium ${
                                      member.role === 'admin'
                                        ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-300'
                                        : 'bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300'
                                    }`}
                                  >
                                    {member.role}
                                  </span>
                                </div>
                              </div>
                            )
                          })}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className='py-8 text-center'>
                      <p className='text-tertiary'>All organization members are already in this group</p>
                    </div>
                  )
                })()}

                {/* Already in group members */}
                {(() => {
                  const existingMembers = members.filter((member) => existingMemberUsernames.has(member.user.username))

                  return existingMembers.length > 0 ? (
                    <div>
                      <h3 className='mb-2 text-sm font-medium text-gray-600 dark:text-gray-400'>
                        Already in Group ({existingMembers.length})
                      </h3>
                      <div className='max-h-40 overflow-y-auto rounded-md border border-gray-200 bg-gray-100 dark:border-gray-700 dark:bg-gray-800/50'>
                        <div className='divide-y divide-gray-200 dark:divide-gray-700'>
                          {existingMembers.map((member) => (
                            <div key={member.user.id} className='flex items-center px-4 py-3 opacity-60'>
                              <div className='mr-3 flex h-4 w-4 items-center justify-center'>
                                <div className='h-2 w-2 rounded-full bg-green-500'></div>
                              </div>
                              <img
                                src={member.user.avatar_urls?.sm || ''}
                                alt={member.user.display_name}
                                className='mr-3 h-8 w-8 shrink-0 rounded-full border border-gray-200 dark:border-gray-600'
                              />
                              <div className='min-w-0 flex-1'>
                                <p className='truncate text-sm font-medium text-gray-500'>{member.user.display_name}</p>
                                <p className='truncate text-xs text-gray-400'>@{member.user.username}</p>
                              </div>
                              <div className='ml-3 flex shrink-0 items-center gap-2'>
                                <span
                                  className={`rounded-full px-2 py-1 text-xs font-medium ${
                                    member.role === 'admin'
                                      ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-300'
                                      : 'bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-300'
                                  }`}
                                >
                                  {member.role}
                                </span>
                                <span className='rounded-full bg-green-100 px-2 py-1 text-xs font-medium text-green-700 dark:bg-green-900/30 dark:text-green-300'>
                                  In Group
                                </span>
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  ) : null
                })()}
              </div>
            )}
          </div>

          {/* Selected members count */}
          {selectedMembers.length > 0 && (
            <div className='mb-4 rounded-md border border-blue-200 bg-blue-50 p-3 dark:border-blue-800 dark:bg-blue-900/20'>
              <p className='text-sm font-medium text-blue-800 dark:text-blue-200'>
                ✓ Selected {selectedMembers.length} member{selectedMembers.length !== 1 ? 's' : ''}
              </p>
            </div>
          )}
        </div>

        {/* Fixed bottom buttons */}
        <div className='shrink-0 rounded-b-lg border-t border-gray-200 bg-gray-50 px-6 py-4 dark:border-gray-700 dark:bg-gray-900/50'>
          <div className='flex justify-end gap-3'>
            <Button variant='plain' onClick={handleClose} disabled={addMembersMutation.isPending}>
              Cancel
            </Button>
            <Button
              variant='primary'
              onClick={handleAddMembersSubmit}
              disabled={selectedMembers.length === 0 || addMembersMutation.isPending}
              className='bg-green-600 hover:bg-green-700'
            >
              {addMembersMutation.isPending
                ? 'Adding...'
                : `Add ${selectedMembers.length} Member${selectedMembers.length !== 1 ? 's' : ''}`}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
