import React, { useState } from 'react'

import { Button } from '@gitmono/ui'

import { useCreateAdminGroup } from '@/hooks/admin/useCreateAdminGroup'

interface CreateGroupDialogProps {
  isOpen: boolean
  onClose: () => void
}

export const CreateGroupDialog = ({ isOpen, onClose }: CreateGroupDialogProps) => {
  const [groupName, setGroupName] = useState('')
  const [groupDescription, setGroupDescription] = useState('')
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const createGroupMutation = useCreateAdminGroup()

  const handleCreateGroup = async () => {
    if (!groupName.trim()) {
      return
    }

    setErrorMessage(null) // Clear previous errors

    try {
      await createGroupMutation.mutateAsync({
        name: groupName.trim(),
        description: groupDescription.trim() || undefined
      })
      // Close dialog and reset form
      handleClose()
    } catch (error: any) {
      // Display error message returned by API
      let errorMsg = 'Failed to create group'

      // Try to extract original error message from InternalError
      if (error?.cause?.response?.data?.err_message) {
        errorMsg = error.cause.response.data.err_message
      } else if (error?.cause?.err_message) {
        errorMsg = error.cause.err_message
      } else if (error?.originalError?.response?.data?.err_message) {
        errorMsg = error.originalError.response.data.err_message
      } else if (error?.response?.data?.err_message) {
        errorMsg = error.response.data.err_message
      } else if (error?.message && error.message !== 'InternalError') {
        errorMsg = error.message
      }

      setErrorMessage(errorMsg)
    }
  }

  const handleClose = () => {
    setGroupName('')
    setGroupDescription('')
    setErrorMessage(null)
    onClose()
  }

  if (!isOpen) return null

  return (
    <div className='bg-opacity-50 fixed inset-0 z-50 flex items-center justify-center bg-black'>
      <div className='bg-primary border-primary w-full max-w-md rounded-lg border p-6 shadow-lg'>
        <h2 className='text-primary mb-4 text-xl font-bold'>Create New Group</h2>

        {/* Error message display */}
        {errorMessage && (
          <div className='mb-4 rounded-md border border-red-200 bg-red-50 p-3 dark:border-red-800 dark:bg-red-900/20'>
            <p className='text-sm font-medium text-red-800 dark:text-red-200'>Error: {errorMessage}</p>
          </div>
        )}

        <div className='mb-4'>
          <label className='text-secondary mb-2 block text-sm font-medium'>
            Group Name <span className='text-red-500'>*</span>
          </label>
          <input
            type='text'
            value={groupName}
            onChange={(e) => setGroupName(e.target.value)}
            className='border-primary bg-secondary text-primary w-full rounded-md border px-3 py-2 focus:ring-2 focus:ring-blue-500 focus:outline-hidden'
            placeholder='Enter group name'
            autoFocus
          />
        </div>

        <div className='mb-6'>
          <label className='text-secondary mb-2 block text-sm font-medium'>Description</label>
          <textarea
            value={groupDescription}
            onChange={(e) => setGroupDescription(e.target.value)}
            className='border-primary bg-secondary text-primary w-full rounded-md border px-3 py-2 focus:ring-2 focus:ring-blue-500 focus:outline-hidden'
            placeholder='Enter group description (optional)'
            rows={3}
          />
        </div>

        <div className='flex justify-end gap-3'>
          <Button variant='plain' onClick={handleClose} disabled={createGroupMutation.isPending}>
            Cancel
          </Button>
          <Button
            variant='primary'
            onClick={handleCreateGroup}
            disabled={!groupName.trim() || createGroupMutation.isPending}
          >
            {createGroupMutation.isPending ? 'Creating...' : 'Create Group'}
          </Button>
        </div>
      </div>
    </div>
  )
}
