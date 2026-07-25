import React from 'react'

import { Button } from '@gitmono/ui'

import { useDeleteAdminGroup } from '@/hooks/admin/useDeleteAdminGroup'

interface DeleteGroupDialogProps {
  groupId: number | null
  onClose: () => void
}

export const DeleteGroupDialog = ({ groupId, onClose }: DeleteGroupDialogProps) => {
  const deleteGroupMutation = useDeleteAdminGroup()

  const confirmDelete = async () => {
    if (groupId === null) return

    try {
      await deleteGroupMutation.mutateAsync(groupId)
      onClose()
    } catch (error) {
      // Error already handled by apiErrorToast
    }
  }

  if (groupId === null) return null

  return (
    <div className='bg-opacity-50 fixed inset-0 z-50 flex items-center justify-center bg-black'>
      <div className='bg-primary border-primary w-full max-w-md rounded-lg border p-6 shadow-lg'>
        <h2 className='text-primary mb-4 text-xl font-bold'>Delete Group</h2>

        <p className='text-secondary mb-6'>Are you sure you want to delete this group? This action cannot be undone.</p>

        <div className='flex justify-end gap-3'>
          <Button variant='plain' onClick={onClose} disabled={deleteGroupMutation.isPending}>
            Cancel
          </Button>
          <Button
            variant='primary'
            onClick={confirmDelete}
            disabled={deleteGroupMutation.isPending}
            className='bg-red-500 hover:bg-red-600'
          >
            {deleteGroupMutation.isPending ? 'Deleting...' : 'Delete'}
          </Button>
        </div>
      </div>
    </div>
  )
}
