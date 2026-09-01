import React, { useEffect, useState } from 'react'

import { UsersIcon } from '@gitmono/ui'

import { AdminGroupEditDialog } from './AdminGroupEditDialog'

interface GroupItem {
  id: string
  name: string
  description?: string | null
  created_at: number
  updated_at: number
}

interface AdminGroupItemProps {
  group: GroupItem
  onDelete: (id: string) => void
  onManageMembers: (id: string) => void
  onUpdate?: () => void
}

export const AdminGroupItem = ({ group, onDelete, onManageMembers, onUpdate }: AdminGroupItemProps) => {
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false)
  const [localGroup, setLocalGroup] = useState(group)
  const [isUpdating, setIsUpdating] = useState(false)
  const createdDate = new Date(localGroup.created_at * 1000).toLocaleDateString()

  useEffect(() => {
    setLocalGroup(group)
    setIsUpdating(false)
  }, [group])

  const handleEditSuccess = (updatedData: { name: string; description: string | null }) => {
    setLocalGroup((prev) => ({
      ...prev,
      name: updatedData.name,
      description: updatedData.description,
      updated_at: Math.floor(Date.now() / 1000)
    }))

    setIsUpdating(true)

    onUpdate?.()

    setTimeout(() => {
      setIsUpdating(false)
    }, 3000)
  }

  return (
    <>
      <div className='border-primary flex items-center justify-between border-b py-6 transition-colors duration-200 last:border-b-0 hover:bg-gray-50 dark:hover:bg-gray-800/50'>
        <div className='flex items-start'>
          <div className='mr-4 rounded-lg bg-blue-100 p-3 dark:bg-blue-900/30'>
            <UsersIcon className='h-6 w-6 text-blue-600 dark:text-blue-400' aria-hidden='true' />
          </div>
          <div className='flex-1'>
            <div className='flex items-center gap-2'>
              <p className='text-primary mb-1 text-lg font-bold'>{localGroup.name}</p>
              {isUpdating && (
                <div className='flex items-center gap-1'>
                  <div className='h-3 w-3 animate-spin rounded-full border border-green-500 border-t-transparent'></div>
                  <span className='text-xs text-green-600'>Updated</span>
                </div>
              )}
            </div>
            <div className='text-tertiary space-y-1 text-sm'>
              {localGroup.description && (
                <p className='max-w-md text-gray-600 dark:text-gray-400'>{localGroup.description}</p>
              )}
              <p className='text-xs text-gray-500 dark:text-gray-500'>Created: {createdDate}</p>
            </div>
          </div>
        </div>

        <div className='flex min-w-0 flex-col gap-3'>
          <div className='flex items-center gap-4'>
            <div className='w-[30%]'>
              <span className='text-sm font-medium text-gray-600 dark:text-gray-400'>Group:</span>
            </div>
            <div className='flex w-[70%] justify-between gap-2'>
              <button
                className='border-primary flex-1 rounded-md border px-4 py-1 text-sm font-semibold text-blue-600 transition-colors duration-200 hover:bg-blue-600 hover:text-white'
                onClick={() => setIsEditDialogOpen(true)}
              >
                Edit
              </button>
              <button
                className='border-primary flex-1 rounded-md border px-4 py-1 text-sm font-semibold text-red-500 transition-colors duration-200 hover:bg-red-500 hover:text-white'
                onClick={() => onDelete(group.id)}
              >
                Delete
              </button>
            </div>
          </div>

          <div className='flex items-center gap-4'>
            <div className='w-[30%]'>
              <span className='text-sm font-medium text-gray-600 dark:text-gray-400'>Members:</span>
            </div>
            <div className='flex w-[70%] justify-between'>
              <button
                className='border-primary w-full rounded-md border px-4 py-1 text-sm font-semibold text-purple-600 transition-colors duration-200 hover:bg-purple-600 hover:text-white'
                onClick={() => onManageMembers(group.id)}
              >
                Manage Members
              </button>
            </div>
          </div>
        </div>
      </div>

      <AdminGroupEditDialog
        open={isEditDialogOpen}
        onOpenChange={setIsEditDialogOpen}
        groupId={localGroup.id}
        onSuccess={handleEditSuccess}
      />
    </>
  )
}
