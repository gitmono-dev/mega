import { useEffect, useMemo, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import * as SettingsSection from 'components/SettingsSection'
import toast from 'react-hot-toast'

import { Vec } from '@gitmono/types/generated'
import * as Dialog from '@gitmono/ui/src/Dialog'

import { useDeleteSidebarById } from '@/hooks/Sidebar/useDeleteSidebarById'
import { useGetSidebarList } from '@/hooks/Sidebar/useGetSidebarList'
import { usePostSidebarSync } from '@/hooks/Sidebar/usePostSidebarSync'

export const MenuPicker = () => {
  const { data, refetch, isFetching } = useGetSidebarList()
  const queryClient = useQueryClient()
  const syncSidebar = usePostSidebarSync()
  const deleteSidebar = useDeleteSidebarById()

  const [items, setItems] = useState<Vec>([])
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null)
  const [isResetting, setIsResetting] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<Vec[number] | null>(null)
  const [fieldErrors, setFieldErrors] = useState<
    Record<number, { public_id?: boolean; label?: boolean; href?: boolean }>
  >({})

  const sortedData = useMemo(() => {
    const list = data?.data || []

    return [...list].sort((a, b) => (a.order_index || 0) - (b.order_index || 0))
  }, [data])

  useEffect(() => {
    if (sortedData.length > 0 && items.length === 0) {
      setItems([...sortedData])
    }
  }, [items.length, sortedData])

  const handleDragStart = (index: number) => {
    setDraggedIndex(index)
  }

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault()
    e.stopPropagation()
    if (draggedIndex === null || draggedIndex === index) return

    const newItems = [...items]
    const draggedItem = newItems[draggedIndex]

    newItems.splice(draggedIndex, 1)
    newItems.splice(index, 0, draggedItem)
    setItems(newItems)
    setDraggedIndex(index)
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
  }

  const handleDragEnd = () => {
    setDraggedIndex(null)
  }

  const handleReset = async () => {
    try {
      setIsResetting(true)
      const result = await refetch()
      const refreshed = result.data?.data || []
      const sorted = [...refreshed].sort((a, b) => (a.order_index || 0) - (b.order_index || 0))

      setItems(sorted)
      queryClient.invalidateQueries({ queryKey: ['sidebar', 'list'] })
    } finally {
      setIsResetting(false)
    }
  }

  const handleApply = async () => {
    const errors: Record<number, { public_id?: boolean; label?: boolean; href?: boolean }> = {}

    items.forEach((item) => {
      const e: { public_id?: boolean; label?: boolean; href?: boolean } = {}

      if (!item.public_id.trim()) e.public_id = true
      if (!item.label.trim()) e.label = true
      if (!item.href.trim()) e.href = true
      if (Object.keys(e).length > 0) errors[item.id] = e
    })
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})

    const withOrder = items.map((item, index) => ({
      ...(item.id > 0 && { id: item.id }),
      label: item.label,
      public_id: item.public_id,
      href: item.href,
      visible: item.visible,
      order_index: index
    }))

    try {
      await syncSidebar.mutateAsync({
        data: withOrder
      })
      queryClient.invalidateQueries({ queryKey: ['sidebar', 'list'] })
      await refetch()
      setItems([])
      toast.success('Saved menu')
    } catch (error) {
      toast.error('Save failed')
    }
  }

  const hasChanges = useMemo(() => {
    if (items.length !== sortedData.length) return true
    return items.some((item, index) => {
      const origin = sortedData[index]

      return (
        item.id !== origin?.id ||
        item.visible !== origin?.visible ||
        item.label !== origin?.label ||
        item.public_id !== origin?.public_id ||
        item.href !== origin?.href ||
        item.order_index !== origin?.order_index
      )
    })
  }, [items, sortedData])

  const handleCreate = () => {
    const tempId = -Date.now()

    setItems((prev) => [
      ...prev,
      {
        id: tempId,
        public_id: '',
        label: '',
        href: '',
        visible: true,
        order_index: prev.length
      }
    ])
    setFieldErrors((prev) => ({
      ...prev,
      [tempId]: { public_id: true, label: true, href: true }
    }))
  }

  return (
    <SettingsSection.Section>
      <SettingsSection.Header>
        <SettingsSection.Title>Menu Order</SettingsSection.Title>
      </SettingsSection.Header>

      <SettingsSection.Description>Drag and drop to reorder menu items</SettingsSection.Description>

      <SettingsSection.Separator />

      <div className='max-w-4xl space-y-3 p-4 pt-2 text-sm'>
        <div className='border-primary overflow-hidden rounded-md border'>
          <table className='w-full text-sm'>
            <thead className='border-primary bg-secondary border-b'>
              <tr>
                <th className='text-primary w-10 px-3 py-2 text-left text-[11px] font-medium'></th>
                <th className='text-primary px-3 py-2 text-left text-[11px] font-medium'>public_id</th>
                <th className='text-primary px-3 py-2 text-left text-[11px] font-medium'>label</th>
                <th className='text-primary px-3 py-2 text-left text-[11px] font-medium'>href</th>
                <th className='text-primary px-3 py-2 text-left text-[11px] font-medium'>visible</th>
                <th className='text-primary w-24 px-3 py-2 text-right text-[11px] font-medium'>actions</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, index) => (
                <tr
                  key={item.id}
                  draggable
                  onDragStart={() => handleDragStart(index)}
                  onDragOver={(e) => handleDragOver(e, index)}
                  onDragEnd={handleDragEnd}
                  onDrop={handleDrop}
                  className={`cursor-move border-b transition-colors ${draggedIndex === index ? 'bg-blue-50 opacity-50 dark:bg-blue-950/20' : 'hover:bg-secondary/50'} `}
                >
                  <td className='text-muted-foreground px-3 py-2'>
                    <svg className='h-3.5 w-3.5' fill='none' stroke='currentColor' viewBox='0 0 24 24'>
                      <path strokeLinecap='round' strokeLinejoin='round' strokeWidth={2} d='M4 8h16M4 16h16' />
                    </svg>
                  </td>
                  <td className='text-muted-foreground px-3 py-2 text-xs'>
                    <input
                      className={`bg-background w-full rounded border px-2 py-1 text-xs focus:ring-1 focus:outline-hidden ${
                        fieldErrors[item.id]?.public_id
                          ? 'border-red-500 focus:border-red-500 focus:ring-red-300'
                          : 'focus:border-primary focus:ring-primary/30 border-transparent'
                      }`}
                      value={item.public_id}
                      onChange={(e) => {
                        const value = e.target.value

                        setItems((prev) => prev.map((row, i) => (i === index ? { ...row, public_id: value } : row)))
                        setFieldErrors((prev) => {
                          const next = { ...prev }

                          if (value.trim()) {
                            const err = { ...(next[item.id] || {}) }

                            delete err.public_id
                            next[item.id] = err
                            if (Object.keys(err).length === 0) delete next[item.id]
                          } else {
                            next[item.id] = { ...(next[item.id] || {}), public_id: true }
                          }
                          return next
                        })
                      }}
                    />
                  </td>
                  <td className='px-3 py-2 font-medium'>
                    <input
                      className={`bg-background w-full rounded border px-2 py-1 text-sm focus:ring-1 focus:outline-hidden ${
                        fieldErrors[item.id]?.label
                          ? 'border-red-500 focus:border-red-500 focus:ring-red-300'
                          : 'focus:border-primary focus:ring-primary/30 border-transparent'
                      }`}
                      value={item.label}
                      onChange={(e) => {
                        const value = e.target.value

                        setItems((prev) => prev.map((row, i) => (i === index ? { ...row, label: value } : row)))
                        setFieldErrors((prev) => {
                          const next = { ...prev }

                          if (value.trim()) {
                            const err = { ...(next[item.id] || {}) }

                            delete err.label
                            next[item.id] = err
                            if (Object.keys(err).length === 0) delete next[item.id]
                          } else {
                            next[item.id] = { ...(next[item.id] || {}), label: true }
                          }
                          return next
                        })
                      }}
                    />
                  </td>
                  <td className='text-muted-foreground px-3 py-2 text-xs'>
                    <input
                      className={`bg-background w-full rounded border px-2 py-1 text-xs focus:ring-1 focus:outline-hidden ${
                        fieldErrors[item.id]?.href
                          ? 'border-red-500 focus:border-red-500 focus:ring-red-300'
                          : 'focus:border-primary focus:ring-primary/30 border-transparent'
                      }`}
                      value={item.href}
                      onChange={(e) => {
                        const value = e.target.value

                        setItems((prev) => prev.map((row, i) => (i === index ? { ...row, href: value } : row)))
                        setFieldErrors((prev) => {
                          const next = { ...prev }

                          if (value.trim()) {
                            const err = { ...(next[item.id] || {}) }

                            delete err.href
                            next[item.id] = err
                            if (Object.keys(err).length === 0) delete next[item.id]
                          } else {
                            next[item.id] = { ...(next[item.id] || {}), href: true }
                          }
                          return next
                        })
                      }}
                    />
                  </td>
                  <td className='px-3 py-2 text-xs'>
                    <button
                      type='button'
                      onClick={() =>
                        setItems((prev) =>
                          prev.map((row, i) => (i === index ? { ...row, visible: !row.visible } : row))
                        )
                      }
                      className={`relative inline-flex h-5 w-10 items-center rounded-full border transition-colors ${
                        item.visible ? 'border-green-500 bg-green-500/80' : 'border-primary bg-secondary'
                      }`}
                      aria-pressed={item.visible}
                    >
                      <span
                        className={`mx-0.5 inline-block h-5 w-5 transform rounded-full bg-white shadow transition ${
                          item.visible ? 'translate-x-6' : 'translate-x-0'
                        }`}
                      />
                    </button>
                  </td>
                  <td className='space-x-2 px-3 py-2 text-right text-xs whitespace-nowrap'>
                    <button
                      type='button'
                      onClick={() => {
                        if (item.id <= 0) {
                          setItems((prev) => prev.filter((row) => row.id !== item.id))
                          setFieldErrors((prev) => {
                            const next = { ...prev }

                            delete next[item.id]
                            return next
                          })
                          return
                        }
                        setPendingDelete(item)
                      }}
                      className='border-primary rounded-md border px-2 py-1 text-xs font-medium text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-50'
                      disabled={deleteSidebar.isPending}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <Dialog.Root
          open={!!pendingDelete}
          onOpenChange={(open) => {
            if (!open) setPendingDelete(null)
          }}
          size='sm'
        >
          <Dialog.Header>
            <Dialog.Title>Delete menu item</Dialog.Title>
            <Dialog.Description>
              {pendingDelete
                ? `Are you sure you want to delete "${pendingDelete.label}" (${pendingDelete.public_id})?`
                : ''}
            </Dialog.Description>
          </Dialog.Header>

          <Dialog.Footer>
            <Dialog.TrailingActions>
              <button
                type='button'
                onClick={() => setPendingDelete(null)}
                className='border-primary bg-primary hover:bg-secondary rounded-md border px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-50'
                disabled={deleteSidebar.isPending}
              >
                Cancel
              </button>
              <button
                type='button'
                onClick={async () => {
                  if (!pendingDelete) return
                  try {
                    await deleteSidebar.mutateAsync({ id: pendingDelete.id })
                    setItems((prev) => prev.filter((row) => row.id !== pendingDelete.id))
                    queryClient.invalidateQueries({ queryKey: ['sidebar', 'list'] })
                    setPendingDelete(null)
                    toast.success('Deleted menu item')
                  } catch (error) {
                    toast.error('Delete failed')
                  }
                }}
                className='rounded-md border border-red-600 bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-50'
                disabled={deleteSidebar.isPending}
              >
                Delete
              </button>
            </Dialog.TrailingActions>
          </Dialog.Footer>
        </Dialog.Root>

        <div className='flex justify-end gap-2 text-sm'>
          <div className='flex-1'>
            <button
              type='button'
              onClick={handleCreate}
              className='border-primary bg-primary/5 hover:bg-primary/10 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50'
            >
              New
            </button>
          </div>

          <div className='flex shrink-0 gap-2'>
            <button
              onClick={handleReset}
              disabled={isResetting || isFetching}
              className='border-primary bg-primary/5 hover:bg-primary/10 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50'
            >
              {isResetting ? 'Resetting...' : 'Reset'}
            </button>

            <button
              onClick={handleApply}
              disabled={!hasChanges || syncSidebar.isPending}
              className='rounded-md border border-blue-600 bg-blue-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50'
            >
              {syncSidebar.isPending ? 'Applying...' : 'Apply'}
            </button>
          </div>
        </div>
      </div>
    </SettingsSection.Section>
  )
}
