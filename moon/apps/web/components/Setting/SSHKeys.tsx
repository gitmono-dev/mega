import React, { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { ListSSHKey } from '@gitmono/types'
import { Button, LoadingSpinner, LockIcon, PlusIcon, TextField } from '@gitmono/ui'
import * as Dialog from '@gitmono/ui/src/Dialog'

import HandleTime from '@/components/ClView/components/HandleTime'
import { useDeleteSSHKeyById } from '@/hooks/useDeleteSSHKeyById'
import { useGetSSHList } from '@/hooks/useGetSSHList'
import { usePostSSHKey } from '@/hooks/usePostSSHKey'
import { legacyApiClient } from '@/utils/queryClient'

const SshKeyItem = ({ keyData }: { keyData: ListSSHKey }) => {
  const { mutate: deleteSSHKey } = useDeleteSSHKeyById()
  const fetchSSHList = legacyApiClient.v1.getApiUserSshList()
  const queryClient = useQueryClient()

  return (
    <div className='border-primary flex items-center justify-between border-b py-4 last:border-b-0'>
      <div className='flex items-start'>
        <LockIcon className='text-quaternary h-6 w-6' aria-hidden='true' />
        <div className='ml-4'>
          <p className='text-primary text-base font-bold'>{keyData.title}</p>
          <p className='text-tertiary mt-1 font-mono text-sm'>{keyData.finger}</p>
          <p className='text-tertiary mt-2 text-xs'>
            <HandleTime created_at={keyData.created_at} />
          </p>
        </div>
      </div>
      <button
        onClick={() =>
          deleteSSHKey(
            { keyId: keyData.id },
            {
              onSuccess: () => {
                queryClient.invalidateQueries({ queryKey: fetchSSHList.requestKey() })
              }
            }
          )
        }
        className='border-primary rounded-md border px-4 py-1 text-sm font-semibold text-red-500 transition-colors duration-200 hover:bg-red-500 hover:text-white'
      >
        Delete
      </button>
    </div>
  )
}

interface NewSSHKeyDialogProps {
  open: boolean
  setOpen: (open: boolean) => void
}

const NewSSHKeyDialog = ({ open, setOpen }: NewSSHKeyDialogProps) => {
  const { mutate: postSSHKey, isPending } = usePostSSHKey()
  const [title, setTitle] = useState('')
  const [sshKey, setSshKey] = useState('')
  const [errors, setErrors] = useState<{ title?: string; sshKey?: string }>({})

  const fetchSSHList = legacyApiClient.v1.getApiUserSshList()
  const queryClient = useQueryClient()

  const handleSubmit = (e?: React.FormEvent | React.MouseEvent) => {
    if (e) e.preventDefault()
    const nextErrors: { title?: string; sshKey?: string } = {}

    if (!title.trim()) nextErrors.title = 'Title is required'
    if (!sshKey.trim()) nextErrors.sshKey = 'SSH key is required'
    setErrors(nextErrors)
    if (Object.keys(nextErrors).length > 0) return
    postSSHKey(
      { data: { title: title.trim(), ssh_key: sshKey } },
      {
        onSuccess: () => {
          setOpen(false)
          setTitle('')
          setSshKey('')
          setErrors({})

          queryClient.invalidateQueries({ queryKey: fetchSSHList.requestKey() })
        }
      }
    )
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen} visuallyHiddenDescription='Add a new SSH key'>
      <Dialog.Title className='w-full p-4'>Add SSH key</Dialog.Title>
      <Dialog.Content className='w-full max-w-md p-4'>
        <div className='mb-4'>
          <TextField autoFocus label='title' value={title} onChange={setTitle} />
          {errors.title && <span className='text-xs text-red-500'>{errors.title}</span>}
        </div>

        <div className='mb-4'>
          <TextField
            placeholder='begins with "ssh-rsa" or "ssh-ed25519"'
            multiline
            minRows={5}
            label='ssh_key'
            value={sshKey}
            onChange={setSshKey}
          />
          {errors.sshKey && <span className='text-xs text-red-500'>{errors.sshKey}</span>}
        </div>
      </Dialog.Content>

      <Dialog.Footer>
        <Dialog.TrailingActions>
          <Button variant='flat' onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button
            variant='primary'
            className='bg-[#1f883d]'
            onClick={handleSubmit}
            disabled={isPending || !title.trim() || !sshKey.trim()}
            loading={isPending}
          >
            Add key
          </Button>
        </Dialog.TrailingActions>
      </Dialog.Footer>
    </Dialog.Root>
  )
}

const SSHKeys = () => {
  const { sshKeys, isLoading } = useGetSSHList()
  const [open, setOpen] = useState(false)

  return (
    <>
      <div className='border-primary bg-primary dark:bg-tertiary text-secondary mx-auto max-w-4xl rounded-lg border p-8 font-sans'>
        <header className='flex items-center justify-between pb-4'>
          <h1 className='text-primary text-3xl font-bold'>SSH keys</h1>
          <Button variant='primary' className='bg-[#1f883d]' leftSlot={<PlusIcon />} onClick={() => setOpen(true)}>
            New SSH key
          </Button>
        </header>

        <p className='mb-8'>
          This is a list of SSH keys associated with your account. Remove any keys that you do not recognize.
        </p>

        <section>
          <h2 className='border-primary text-primary border-b pb-2 text-xl font-semibold'>Authentication keys</h2>
          {isLoading ? (
            <div className='flex h-[400px] items-center justify-center'>
              <LoadingSpinner />
            </div>
          ) : (
            <div>
              {sshKeys?.map((key) => (
                <SshKeyItem key={key.id} keyData={key} />
              ))}
            </div>
          )}
        </section>
      </div>
      <NewSSHKeyDialog open={open} setOpen={setOpen} />
    </>
  )
}

export default SSHKeys
