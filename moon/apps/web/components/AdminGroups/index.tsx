import React, { useState } from 'react'

import { Button, PlusIcon } from '@gitmono/ui'

import { useAdminGroupsList } from '@/hooks/admin/useAdminGroupsList'

import { AdminGroupsList } from './AdminGroupsList'
import { CreateGroupDialog } from './CreateGroupDialog'
import { DeleteGroupDialog } from './DeleteGroupDialog'
import { GroupMembersDialog } from './GroupMembersDialog'

export const AdminGroups = () => {
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false)
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null)
  const [manageMembersGroupId, setManageMembersGroupId] = useState<string | null>(null)

  const { data, isLoading, isError } = useAdminGroupsList({
    pagination: { page: 1, per_page: 20 },
    additional: {}
  })

  const groups = data?.data?.items || []
  const total = data?.data?.total || 0

  const handleDeleteGroup = (groupId: string) => {
    setDeleteConfirmId(groupId)
  }

  const handleManageMembers = (groupId: string) => {
    setManageMembersGroupId(groupId)
  }

  return (
    <div className='border-primary bg-primary dark:bg-tertiary text-secondary mx-auto max-w-4xl rounded-lg border p-8 font-sans'>
      <header className='flex items-center justify-between pb-4'>
        <h1 className='text-primary text-3xl font-bold'>Admin Groups</h1>
        <Button
          variant='primary'
          className='bg-[#1f883d]'
          leftSlot={<PlusIcon />}
          onClick={() => setIsCreateDialogOpen(true)}
        >
          New Group
        </Button>
      </header>

      <p className='mb-8'>
        Manage admin groups and their permissions. Only administrators can view and manage this list.
      </p>

      <AdminGroupsList
        groups={groups}
        total={total}
        isLoading={isLoading}
        isError={isError}
        onDelete={handleDeleteGroup}
        onManageMembers={handleManageMembers}
      />

      <CreateGroupDialog isOpen={isCreateDialogOpen} onClose={() => setIsCreateDialogOpen(false)} />

      <DeleteGroupDialog groupId={deleteConfirmId} onClose={() => setDeleteConfirmId(null)} />

      <GroupMembersDialog
        groupId={manageMembersGroupId}
        groupName={groups.find((g) => g.id === manageMembersGroupId)?.name}
        onClose={() => setManageMembersGroupId(null)}
      />
    </div>
  )
}
