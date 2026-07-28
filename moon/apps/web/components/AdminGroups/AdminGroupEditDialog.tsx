import React, { useEffect, useState } from 'react'
import toast from 'react-hot-toast'

import { PermissionValue } from '@gitmono/types'
import { Button, Dialog, TextField } from '@gitmono/ui'

import { useDeleteResourcePermissions } from '@/hooks/admin/useDeleteResourcePermissions'
import { useGetAdminGroupById } from '@/hooks/admin/useGetAdminGroupById'
import { usePostResourcePermissions } from '@/hooks/admin/usePostResourcePermissions'
import { useUpdateAdminGroup } from '@/hooks/admin/useUpdateAdminGroup'
import { useGetMegaForMeNotes } from '@/hooks/useGetMegaForMeNotes'
import { legacyApiClient } from '@/utils/queryClient'

interface AdminGroupEditDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  groupId: number | null
  onSuccess?: (updatedData: { name: string; description: string | null }) => void
}

export function AdminGroupEditDialog({ open, onOpenChange, groupId, onSuccess }: AdminGroupEditDialogProps) {
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [isFormReady, setIsFormReady] = useState(false)
  const [activeTab, setActiveTab] = useState<'basic' | 'resources'>('basic')

  const updateGroup = useUpdateAdminGroup()
  const postResourcePermissions = usePostResourcePermissions()
  const deleteResourcePermissions = useDeleteResourcePermissions()

  const [resourcePermissions, setResourcePermissions] = useState<
    Record<string, { read: boolean; write: boolean; admin: boolean }>
  >({})

  const [savingResources, setSavingResources] = useState<Set<string>>(new Set())

  const [deletingResources, setDeletingResources] = useState<Set<string>>(new Set())

  const handlePermissionToggle = (resourceId: string, type: 'read' | 'write' | 'admin') => {
    setResourcePermissions((prev) => {
      const current = prev[resourceId] || { read: false, write: false, admin: false }

      return {
        ...prev,
        [resourceId]: {
          ...current,
          [type]: !current[type]
        }
      }
    })
  }

  const handleDeletePermission = async (resourceId: string) => {
    if (!groupId) return

    setDeletingResources((prev) => new Set(prev).add(resourceId))

    try {
      await deleteResourcePermissions.mutateAsync({
        resourceType: 'note',
        resourceId: resourceId
      })

      setResourcePermissions((prev) => {
        const newPerms = { ...prev }

        delete newPerms[resourceId]
        return newPerms
      })

      toast.success('Permission removed')
    } catch {
      toast.error('Failed to remove permission')
    } finally {
      setDeletingResources((prev) => {
        const newSet = new Set(prev)

        newSet.delete(resourceId)
        return newSet
      })
    }
  }

  const handleSaveResourcePermission = async (resourceId: string) => {
    if (!groupId) return

    const perms = resourcePermissions[resourceId]

    if (!perms || (!perms.read && !perms.write && !perms.admin)) {
      toast.error('Please select at least one permission')
      return
    }

    setSavingResources((prev) => new Set(prev).add(resourceId))

    try {
      const permissions = []

      if (perms.read) permissions.push({ group_id: groupId, permission: PermissionValue.Read })
      if (perms.write) permissions.push({ group_id: groupId, permission: PermissionValue.Write })
      if (perms.admin) permissions.push({ group_id: groupId, permission: PermissionValue.Admin })

      await postResourcePermissions.mutateAsync({
        resourceType: 'note',
        resourceId: resourceId,
        data: {
          permissions
        }
      })

      toast.success('Permission saved!')
    } catch {
      toast.error('Failed to save permission')
    } finally {
      setSavingResources((prev) => {
        const newSet = new Set(prev)

        newSet.delete(resourceId)
        return newSet
      })
    }
  }

  const hasAnyPermission = (resourceId: string) => {
    const perms = resourcePermissions[resourceId]

    return perms && (perms.read || perms.write || perms.admin)
  }

  const { data: groupData, isLoading, error } = useGetAdminGroupById(groupId || 0, { enabled: !!groupId && open })

  const {
    data: resourcesData,
    isLoading: isLoadingResources,
    fetchNextPage,
    hasNextPage
  } = useGetMegaForMeNotes({
    enabled: open && activeTab === 'resources'
  })

  const allResources = resourcesData?.pages.flatMap((page) => page.data) || []

  useEffect(() => {
    if (groupData?.data && open) {
      setName(groupData.data.name)
      setDescription(groupData.data.description || '')
      setIsFormReady(true)
    }
  }, [groupData, open])

  useEffect(() => {
    if (!open) {
      setName('')
      setDescription('')
      setIsFormReady(false)
      setActiveTab('basic')
      setResourcePermissions({})
      setSavingResources(new Set())
      setDeletingResources(new Set())
    }
  }, [open])

  useEffect(() => {
    if (!groupId || activeTab !== 'resources' || allResources.length === 0) return

    const loadPermissions = async () => {
      const newPermissions: Record<string, { read: boolean; write: boolean; admin: boolean }> = {}

      await Promise.all(
        allResources.map(async (resource) => {
          try {
            const response = await legacyApiClient.v1.getApiAdminResourcesPermissions().request('note', resource.id)

            if (response?.data && Array.isArray(response.data)) {
              const groupPermissions = response.data.filter((p) => p.group_id === groupId)

              if (groupPermissions.length > 0) {
                const perms = { read: false, write: false, admin: false }

                groupPermissions.forEach((p) => {
                  if (p.permission === 'read') perms.read = true
                  if (p.permission === 'write') perms.write = true
                  if (p.permission === 'admin') perms.admin = true
                })

                newPermissions[resource.id] = perms
              }
            }
          } catch {
            // Silently ignore errors for individual resources
          }
        })
      )

      if (Object.keys(newPermissions).length > 0) {
        setResourcePermissions((prev) => {
          const hasChanges = Object.keys(newPermissions).some((key) => {
            const prevPerms = prev[key]
            const newPerms = newPermissions[key]

            return (
              !prevPerms ||
              prevPerms.read !== newPerms.read ||
              prevPerms.write !== newPerms.write ||
              prevPerms.admin !== newPerms.admin
            )
          })

          return hasChanges ? { ...prev, ...newPermissions } : prev
        })
      }
    }

    loadPermissions()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupId, activeTab, allResources.length])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (!groupId || !groupData?.data) return

    try {
      await updateGroup.mutateAsync({
        groupId: groupId,
        data: {
          name,
          description: description || null
        }
      })

      toast.success('Group updated successfully!')

      onSuccess?.({
        name,
        description: description || null
      })

      onOpenChange(false)
    } catch {
      toast.error('Failed to update group. Please try again.')
    }
  }

  if (!groupId) return null

  if (isLoading || !isFormReady) {
    return (
      <Dialog.Root open={open} onOpenChange={onOpenChange} size='3xl'>
        <Dialog.Header>
          <div className='flex items-center border-b border-gray-200 dark:border-gray-700'>
            <button className='relative px-4 py-3 text-sm font-medium text-blue-600 dark:text-blue-400'>
              Basic Info
              <div className='absolute right-0 bottom-0 left-0 h-0.5 bg-blue-600 dark:bg-blue-400'></div>
            </button>
            <button className='px-4 py-3 text-sm font-medium text-gray-600 dark:text-gray-400'>Permissions</button>
          </div>
          <Dialog.CloseButton />
        </Dialog.Header>
        <Dialog.Content className='!max-w-none'>
          <div className='flex items-center justify-center py-12'>
            <div className='flex flex-col items-center space-y-4'>
              <div className='h-8 w-8 animate-spin rounded-full border-2 border-gray-300 border-t-blue-600'></div>
              <div className='text-sm text-gray-500'>Loading group details...</div>
            </div>
          </div>
        </Dialog.Content>
        <Dialog.Footer>
          <Dialog.TrailingActions>
            <Button variant='primary' disabled>
              Loading...
            </Button>
          </Dialog.TrailingActions>
        </Dialog.Footer>
      </Dialog.Root>
    )
  }

  if (error || !groupData?.data) {
    return (
      <Dialog.Root open={open} onOpenChange={onOpenChange} size='3xl'>
        <Dialog.Header>
          <div className='flex items-center border-b border-gray-200 dark:border-gray-700'>
            <button className='relative px-4 py-3 text-sm font-medium text-blue-600 dark:text-blue-400'>
              Basic Info
              <div className='absolute right-0 bottom-0 left-0 h-0.5 bg-blue-600 dark:bg-blue-400'></div>
            </button>
            <button className='px-4 py-3 text-sm font-medium text-gray-600 dark:text-gray-400'>Permissions</button>
          </div>
          <Dialog.CloseButton />
        </Dialog.Header>
        <Dialog.Content className='!max-w-none'>
          <div className='flex items-center justify-center py-12'>
            <div className='flex flex-col items-center space-y-4'>
              <div className='text-red-500'>
                <svg className='h-8 w-8' fill='none' stroke='currentColor' viewBox='0 0 24 24'>
                  <path
                    strokeLinecap='round'
                    strokeLinejoin='round'
                    strokeWidth={2}
                    d='M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z'
                  />
                </svg>
              </div>
              <div className='text-sm text-red-500'>Failed to load group details</div>
              <Button variant='primary' onClick={() => window.location.reload()}>
                Retry
              </Button>
            </div>
          </div>
        </Dialog.Content>
        <Dialog.Footer>
          <Dialog.TrailingActions>
            <Button variant='plain' onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </Dialog.TrailingActions>
        </Dialog.Footer>
      </Dialog.Root>
    )
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange} size='3xl'>
      <Dialog.Header>
        <div className='flex items-center border-b border-gray-200 dark:border-gray-700'>
          <button
            onClick={() => setActiveTab('basic')}
            className={`relative px-4 py-3 text-sm font-medium transition-colors ${
              activeTab === 'basic'
                ? 'text-blue-600 dark:text-blue-400'
                : 'text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-gray-200'
            }`}
          >
            Basic Info
            {activeTab === 'basic' && (
              <div className='absolute right-0 bottom-0 left-0 h-0.5 bg-blue-600 dark:bg-blue-400'></div>
            )}
          </button>
          <button
            onClick={() => setActiveTab('resources')}
            className={`relative px-4 py-3 text-sm font-medium transition-colors ${
              activeTab === 'resources'
                ? 'text-blue-600 dark:text-blue-400'
                : 'text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-gray-200'
            }`}
          >
            Permissions
            {activeTab === 'resources' && (
              <div className='absolute right-0 bottom-0 left-0 h-0.5 bg-blue-600 dark:bg-blue-400'></div>
            )}
          </button>
        </div>
        <Dialog.CloseButton />
      </Dialog.Header>

      <form onSubmit={handleSubmit}>
        <Dialog.Content className='!max-w-none'>
          {activeTab === 'basic' && (
            <div className='space-y-6'>
              <div className='space-y-4'>
                <h3 className='text-sm font-semibold text-gray-900 dark:text-gray-100'>Group Details</h3>

                <div>
                  <label
                    htmlFor='group-name'
                    className='mb-1.5 block text-sm font-medium text-gray-700 dark:text-gray-300'
                  >
                    Group Name <span className='text-red-500'>*</span>
                  </label>
                  <TextField
                    id='group-name'
                    value={name}
                    onChange={setName}
                    placeholder='Enter group name'
                    required
                    data-autofocus='true'
                  />
                </div>

                <div>
                  <label
                    htmlFor='group-description'
                    className='mb-1.5 block text-sm font-medium text-gray-700 dark:text-gray-300'
                  >
                    Description
                  </label>
                  <textarea
                    id='group-description'
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    placeholder='Enter group description'
                    rows={3}
                    className='w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 focus:outline-hidden dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100'
                  />
                </div>
              </div>
            </div>
          )}

          {activeTab === 'resources' && (
            <div className='space-y-4'>
              {isLoadingResources && allResources.length === 0 ? (
                <div className='flex items-center justify-center py-16'>
                  <div className='flex flex-col items-center gap-3'>
                    <div className='h-10 w-10 animate-spin rounded-full border-4 border-gray-200 border-t-blue-600 dark:border-gray-700 dark:border-t-blue-400'></div>
                    <span className='text-sm font-medium text-gray-600 dark:text-gray-400'>Loading resources...</span>
                  </div>
                </div>
              ) : (
                <>
                  <div className='overflow-hidden rounded-lg border border-gray-200 shadow-xs dark:border-gray-700'>
                    <div className='overflow-x-auto'>
                      <table className='w-full min-w-full table-fixed'>
                        <thead className='border-b border-gray-200 bg-gradient-to-b from-gray-50 to-gray-100 dark:border-gray-700 dark:from-gray-800 dark:to-gray-800/80'>
                          <tr>
                            <th className='w-[32%] px-4 py-3 text-left text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Resource
                            </th>
                            <th className='w-[16%] px-3 py-3 text-left text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Project
                            </th>
                            <th className='w-[10%] px-2 py-3 text-center text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Read
                            </th>
                            <th className='w-[10%] px-2 py-3 text-center text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Write
                            </th>
                            <th className='w-[10%] px-2 py-3 text-center text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Admin
                            </th>
                            <th className='w-[22%] px-3 py-3 text-center text-xs font-semibold tracking-wider text-gray-700 uppercase dark:text-gray-300'>
                              Actions
                            </th>
                          </tr>
                        </thead>
                        <tbody className='divide-y divide-gray-100 bg-white dark:divide-gray-800 dark:bg-gray-900'>
                          {allResources.length === 0 ? (
                            <tr>
                              <td colSpan={6} className='px-4 py-16 text-center'>
                                <div className='flex flex-col items-center gap-3'>
                                  <div className='rounded-full bg-gray-100 p-4 dark:bg-gray-800'>
                                    <svg
                                      className='h-10 w-10 text-gray-400 dark:text-gray-600'
                                      fill='none'
                                      stroke='currentColor'
                                      viewBox='0 0 24 24'
                                    >
                                      <path
                                        strokeLinecap='round'
                                        strokeLinejoin='round'
                                        strokeWidth={1.5}
                                        d='M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z'
                                      />
                                    </svg>
                                  </div>
                                  <div>
                                    <div className='text-sm font-medium text-gray-900 dark:text-gray-100'>
                                      No resources found
                                    </div>
                                    <div className='text-xs text-gray-500 dark:text-gray-400'>
                                      There are no resources available to manage
                                    </div>
                                  </div>
                                </div>
                              </td>
                            </tr>
                          ) : (
                            allResources.map((resource) => {
                              const perms = resourcePermissions[resource.id] || {
                                read: false,
                                write: false,
                                admin: false
                              }
                              const hasPerms = hasAnyPermission(resource.id)
                              const isSaving = savingResources.has(resource.id)
                              const isDeleting = deletingResources.has(resource.id)

                              return (
                                <tr
                                  key={resource.id}
                                  className='group transition-colors hover:bg-gray-50 dark:hover:bg-gray-800/50'
                                >
                                  <td className='px-4 py-3'>
                                    <div className='flex flex-col gap-0.5'>
                                      <span
                                        className='truncate text-sm font-medium text-gray-900 dark:text-gray-100'
                                        title={resource.title}
                                      >
                                        {resource.title}
                                      </span>
                                      <div className='flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-500'>
                                        <span className='flex items-center gap-0.5'>
                                          <svg
                                            className='h-3 w-3'
                                            fill='none'
                                            stroke='currentColor'
                                            viewBox='0 0 24 24'
                                          >
                                            <path
                                              strokeLinecap='round'
                                              strokeLinejoin='round'
                                              strokeWidth={2}
                                              d='M8 10h.01M12 10h.01M16 10h.01M9 16H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-5l-5 5v-5z'
                                            />
                                          </svg>
                                          {resource.comments_count}
                                        </span>
                                      </div>
                                    </div>
                                  </td>

                                  <td className='px-3 py-3'>
                                    <span
                                      className='inline-flex items-center truncate rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-700 dark:bg-gray-800 dark:text-gray-300'
                                      title={resource.project?.name || 'No Project'}
                                    >
                                      {resource.project?.name || 'None'}
                                    </span>
                                  </td>

                                  <td className='px-2 py-3 text-center'>
                                    <label className='inline-flex cursor-pointer items-center justify-center'>
                                      <input
                                        type='checkbox'
                                        checked={perms.read}
                                        onChange={() => handlePermissionToggle(resource.id, 'read')}
                                        className='h-4 w-4 cursor-pointer rounded border-gray-300 text-blue-600 transition-all focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 dark:border-gray-600 dark:bg-gray-700 dark:focus:ring-offset-gray-900'
                                      />
                                    </label>
                                  </td>

                                  <td className='px-2 py-3 text-center'>
                                    <label className='inline-flex cursor-pointer items-center justify-center'>
                                      <input
                                        type='checkbox'
                                        checked={perms.write}
                                        onChange={() => handlePermissionToggle(resource.id, 'write')}
                                        className='h-4 w-4 cursor-pointer rounded border-gray-300 text-green-600 transition-all focus:ring-2 focus:ring-green-500 focus:ring-offset-1 dark:border-gray-600 dark:bg-gray-700 dark:focus:ring-offset-gray-900'
                                      />
                                    </label>
                                  </td>

                                  <td className='px-2 py-3 text-center'>
                                    <label className='inline-flex cursor-pointer items-center justify-center'>
                                      <input
                                        type='checkbox'
                                        checked={perms.admin}
                                        onChange={() => handlePermissionToggle(resource.id, 'admin')}
                                        className='h-4 w-4 cursor-pointer rounded border-gray-300 text-purple-600 transition-all focus:ring-2 focus:ring-purple-500 focus:ring-offset-1 dark:border-gray-600 dark:bg-gray-700 dark:focus:ring-offset-gray-900'
                                      />
                                    </label>
                                  </td>

                                  <td className='px-3 py-3'>
                                    <div className='flex items-center justify-center gap-1.5'>
                                      {hasPerms ? (
                                        <>
                                          <button
                                            onClick={() => handleSaveResourcePermission(resource.id)}
                                            disabled={isSaving || isDeleting}
                                            className='inline-flex items-center gap-1 rounded-md bg-blue-600 px-2.5 py-1 text-xs font-semibold text-white shadow-xs transition-all hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-blue-500 dark:hover:bg-blue-600'
                                            title='Save permissions for this resource'
                                          >
                                            {isSaving ? (
                                              <>
                                                <div className='h-3 w-3 animate-spin rounded-full border-2 border-white border-t-transparent'></div>
                                                Saving...
                                              </>
                                            ) : (
                                              <>
                                                <svg
                                                  className='h-3 w-3'
                                                  fill='none'
                                                  stroke='currentColor'
                                                  viewBox='0 0 24 24'
                                                >
                                                  <path
                                                    strokeLinecap='round'
                                                    strokeLinejoin='round'
                                                    strokeWidth={2}
                                                    d='M5 13l4 4L19 7'
                                                  />
                                                </svg>
                                                Save
                                              </>
                                            )}
                                          </button>
                                          <button
                                            onClick={() => handleDeletePermission(resource.id)}
                                            disabled={isSaving || isDeleting}
                                            className='inline-flex items-center gap-0.5 rounded-md bg-red-50 px-2 py-1 text-xs font-semibold text-red-700 transition-all hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/30'
                                            title='Clear all permissions'
                                          >
                                            {isDeleting ? (
                                              <>
                                                <div className='h-3 w-3 animate-spin rounded-full border-2 border-red-700 border-t-transparent dark:border-red-400 dark:border-t-transparent'></div>
                                                Clearing...
                                              </>
                                            ) : (
                                              <>
                                                <svg
                                                  className='h-3 w-3'
                                                  fill='none'
                                                  stroke='currentColor'
                                                  viewBox='0 0 24 24'
                                                >
                                                  <path
                                                    strokeLinecap='round'
                                                    strokeLinejoin='round'
                                                    strokeWidth={2}
                                                    d='M6 18L18 6M6 6l12 12'
                                                  />
                                                </svg>
                                                Clear
                                              </>
                                            )}
                                          </button>
                                        </>
                                      ) : (
                                        <span className='text-sm text-gray-300 dark:text-gray-700'>—</span>
                                      )}
                                    </div>
                                  </td>
                                </tr>
                              )
                            })
                          )}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  {hasNextPage && (
                    <div className='flex justify-center pt-2'>
                      <Button variant='plain' onClick={() => fetchNextPage()} disabled={isLoadingResources} size='sm'>
                        {isLoadingResources ? (
                          <span className='flex items-center gap-2'>
                            <div className='h-3 w-3 animate-spin rounded-full border-2 border-gray-300 border-t-blue-600'></div>
                            Loading...
                          </span>
                        ) : (
                          'Load More'
                        )}
                      </Button>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </Dialog.Content>

        <Dialog.Footer>
          <Dialog.TrailingActions>
            {activeTab === 'basic' && (
              <Button type='submit' variant='primary' disabled={!name.trim() || updateGroup.isPending}>
                {updateGroup.isPending ? 'Saving...' : 'Save Changes'}
              </Button>
            )}
          </Dialog.TrailingActions>
        </Dialog.Footer>
      </form>
    </Dialog.Root>
  )
}
