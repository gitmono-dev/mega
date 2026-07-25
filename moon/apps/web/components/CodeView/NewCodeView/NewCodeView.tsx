import { useState } from 'react'
import toast from 'react-hot-toast'

import { Button } from '@gitmono/ui/Button'
import { Dialog } from '@gitmono/ui/Dialog'
import { Select, SelectTrigger, SelectValue } from '@gitmono/ui/Select'

import { useCreateEntry } from '@/hooks/useCreateEntry'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'

import MarkdownEditor from './MarkdownEditor'
import PathInput from './PathInput'

interface NewCodeViewProps {
  currentPath?: string
  onClose?: () => void
  defaultType?: 'folder' | 'file'
}

const NewCodeView = ({ currentPath = '', onClose, defaultType = 'file' }: NewCodeViewProps) => {
  const [path, setPath] = useState(currentPath)
  const [name, setName] = useState('')

  const [skipBuild, setSkipBuild] = useState(false)

  const [fileType, setFileType] = useState<'folder' | 'file'>(defaultType)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [content, setContent] = useState('')
  const createEntryHook = useCreateEntry()
  const { data: currentUser } = useGetCurrentUser()

  const handleSubmit = () => {
    const fullPath = path
    const lastSlashIndex = fullPath.lastIndexOf('/')

    const parentPath = lastSlashIndex > 0 ? fullPath.substring(0, lastSlashIndex) : lastSlashIndex === 0 ? '/' : ''
    const normalizedParentPath = parentPath === '' ? '/' : parentPath.startsWith('/') ? parentPath : `/${parentPath}`

    // Use explicit name if provided, otherwise extract from path
    const fileName = name || (lastSlashIndex >= 0 ? fullPath.substring(lastSlashIndex + 1) : fullPath)

    createEntryHook.mutate(
      {
        name: fileName,
        path: normalizedParentPath,
        is_directory: fileType === 'folder',
        content: fileType === 'file' ? content : '',
        author_email: currentUser?.email,
        author_username: currentUser?.username,
        mode: 'force_create',
        skip_build: skipBuild
      },
      {
        onSuccess: async () => {
          toast.success('Create Change List Success!')
          setDialogOpen(false)
          onClose?.()
        },
        onError: (error: any) => {
          // Try to read a useful message from the error object
          const msg =
            error?.message ||
            (error?.response && error.response.data && error.response.data.message) ||
            'Create failed. Please try again.'

          toast.error(msg)
        }
      }
    )
  }

  const handleCommitClick = () => {
    setDialogOpen(true)
    setSkipBuild(false)
  }

  const handleDialogClose = (open: boolean) => {
    setDialogOpen(open)
    if (!open) {
      setSkipBuild(false)
    }
  }

  return (
    <div className='flex h-full w-full flex-col gap-2'>
      <div className='flex min-h-14 w-full items-center justify-between pr-4 pl-2'>
        <PathInput pathState={[path, setPath]} nameState={[name, setName]} />
        <div className='flex gap-2'>
          <Button disabled={name === ''} onClick={handleCommitClick}>
            Create CL
          </Button>
          <Select
            typeAhead
            options={[
              { value: 'folder', label: 'Folder' },
              { value: 'file', label: 'File' }
            ]}
            value={fileType}
            onChange={(value) => {
              setFileType(value as 'folder' | 'file')
            }}
          >
            <SelectTrigger>
              <SelectValue placeholder='Select Create Type' />
            </SelectTrigger>
          </Select>
        </div>
      </div>

      {/*The second parameter of MarkdownEditor is to disable the editor, which is currently hidden directly.  */}
      {fileType === 'file' && (
        <div className='w-full flex-1 overflow-y-auto'>
          <MarkdownEditor contentState={[content, setContent]} disabled={false} />
        </div>
      )}

      <Dialog.Root open={dialogOpen} onOpenChange={handleDialogClose}>
        <Dialog.Content>
          <Dialog.CloseButton />
          <Dialog.Header>
            <Dialog.Title>Create {fileType === 'folder' ? 'Folder' : 'File'}</Dialog.Title>
          </Dialog.Header>

          <div className='flex flex-col gap-4 py-4'>
            <div className='flex items-center gap-2'>
              <input
                type='checkbox'
                id='skipBuild_creat'
                checked={skipBuild}
                onChange={(e) => setSkipBuild(e.target.checked)}
                className='h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500'
                disabled={createEntryHook.isPending}
              />
              <label htmlFor='skipBuild_creat' className='text-sm font-medium text-gray-700'>
                Skip automatic build after commit
              </label>
            </div>
          </div>

          <Dialog.Footer>
            <Dialog.TrailingActions>
              <Button variant='flat' onClick={() => handleDialogClose(false)} disabled={createEntryHook.isPending}>
                Cancel
              </Button>
              <Button onClick={handleSubmit} disabled={createEntryHook.isPending}>
                {createEntryHook.isPending ? 'Creating...' : 'Confirm'}
              </Button>
            </Dialog.TrailingActions>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Root>
    </div>
  )
}

export default NewCodeView
