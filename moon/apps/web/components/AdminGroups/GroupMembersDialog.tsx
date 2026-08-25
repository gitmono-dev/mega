import React, { useState } from 'react'

import { Button, LoadingSpinner, PlusIcon, TrashIcon } from '@gitmono/ui'

import { useMemberMap } from '@/components/Issues/utils/sideEffect'
import { useAdminGroupMembersList } from '@/hooks/admin/useAdminGroupMembersList'
import { useDeleteAdminGroupMember } from '@/hooks/admin/useDeleteAdminGroupMember'
import { megaUserHandle } from '@/utils/megaUser'

import { AddMembersDialog } from './AddMembersDialog'

interface GroupMembersDialogProps {
  groupId: number | null
  groupName?: string
  onClose: () => void
}

export const GroupMembersDialog = ({ groupId, groupName, onClose }: GroupMembersDialogProps) => {
  const [deletingUsername, setDeletingUsername] = useState<string | null>(null)
  const [showAddMembersDialog, setShowAddMembersDialog] = useState(false)
  const memberMap = useMemberMap()

  // Get current group's member list
  const { data: groupMembersData, isLoading } = useAdminGroupMembersList(groupId || 0, {
    pagination: { page: 1, per_page: 1000 }, // Get all group members
    additional: {}
  })

  const deleteMemberMutation = useDeleteAdminGroupMember()

  const handleDeleteMember = async (username: string) => {
    if (groupId === null) return

    setDeletingUsername(username)
    try {
      await deleteMemberMutation.mutateAsync({
        groupId: groupId,
        username: username
      })
    } catch {
      // Error already handled by apiErrorToast
    } finally {
      setDeletingUsername(null)
    }
  }

  const members = groupMembersData?.data?.items || []

  if (groupId === null) return null

  return (
    <div className='fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4'>
      <div className='bg-primary border-primary flex max-h-[90vh] w-full max-w-2xl flex-col rounded-lg border shadow-xl'>
        {/* Fixed header */}
        <div className='shrink-0 border-b border-gray-200 px-6 py-4 dark:border-gray-700'>
          <div className='flex items-center justify-between'>
            <div>
              <h2 className='text-primary text-xl font-bold'>Group Members{groupName ? ` - ${groupName}` : ''}</h2>
              <p className='text-tertiary mt-1 text-sm'>
                {members.length} member{members.length !== 1 ? 's' : ''} in this group
              </p>
            </div>
            <Button
              variant='primary'
              size='sm'
              leftSlot={<PlusIcon />}
              onClick={() => setShowAddMembersDialog(true)}
              className='bg-green-600 hover:bg-green-700'
            >
              Add Members
            </Button>
          </div>
        </div>

        {/* Scrollable content area */}
        <div className='flex-1 overflow-y-auto px-6 py-4'>
          {isLoading ? (
            <div className='flex items-center justify-center py-12'>
              <LoadingSpinner />
              <span className='text-tertiary ml-2'>Loading members...</span>
            </div>
          ) : members.length === 0 ? (
            <div className='py-12 text-center'>
              <p className='text-tertiary mb-3'>No members in this group</p>
            </div>
          ) : (
            <div className='space-y-2'>
              {members.map((member) => {
                const synced = memberMap.get(member.username)
                const displayName = megaUserHandle(synced?.user, member.username)
                const handle = synced?.user.username || member.username
                const avatarUrl = synced?.user.avatar_urls?.sm || synced?.user.avatar_urls?.base
                const initial = (displayName || handle).charAt(0).toUpperCase()

                return (
                  <div
                    key={member.id}
                    className='flex items-center rounded-lg border border-gray-200 bg-gray-50 px-4 py-3 transition-colors hover:bg-gray-100 dark:border-gray-700 dark:bg-gray-900/50 dark:hover:bg-gray-800'
                  >
                    {avatarUrl ? (
                      <img
                        src={avatarUrl}
                        alt={displayName}
                        className='mr-4 h-10 w-10 shrink-0 rounded-full border border-gray-200 dark:border-gray-600'
                      />
                    ) : (
                      <div className='mr-4 flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-gray-200 bg-gray-300 dark:border-gray-600 dark:bg-gray-600'>
                        <span className='text-sm font-medium text-gray-600 dark:text-gray-300'>{initial}</span>
                      </div>
                    )}
                    <div className='min-w-0 flex-1'>
                      <p className='text-primary truncate text-sm font-medium'>{displayName}</p>
                      <p className='text-tertiary truncate text-xs'>
                        @{handle}
                        {' · '}
                        Joined: {new Date(member.joined_at * 1000).toLocaleDateString()}
                      </p>
                    </div>
                    <div className='flex shrink-0 items-center gap-3'>
                      <Button
                        variant='plain'
                        size='sm'
                        onClick={() => handleDeleteMember(member.username)}
                        disabled={deletingUsername === member.username || deleteMemberMutation.isPending}
                        className='text-red-600 hover:bg-red-50 hover:text-red-800 dark:hover:bg-red-900/20'
                      >
                        {deletingUsername === member.username ? <LoadingSpinner /> : <TrashIcon className='h-4 w-4' />}
                      </Button>
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </div>

        {/* Fixed bottom buttons */}
        <div className='shrink-0 rounded-b-lg border-t border-gray-200 bg-gray-50 px-6 py-4 dark:border-gray-700 dark:bg-gray-900/50'>
          <div className='flex justify-end'>
            <Button variant='plain' onClick={onClose}>
              Close
            </Button>
          </div>
        </div>
      </div>

      {/* Add Members Dialog */}
      {showAddMembersDialog && <AddMembersDialog groupId={groupId} onClose={() => setShowAddMembersDialog(false)} />}
    </div>
  )
}
