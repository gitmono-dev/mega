import React, { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { GpgKey } from '@gitmono/types'
import { Button, LoadingSpinner, LockIcon, PlusIcon, TextField } from '@gitmono/ui'
import * as Dialog from '@gitmono/ui/src/Dialog'

import HandleTime from '@/components/ClView/components/HandleTime'
import { DateAndTimePicker } from '@/components/DateAndTimePicker'
import { useDeleteGPGKeyById } from '@/hooks/useDeleteGPGKeyById'
import { useGetGPGList } from '@/hooks/useGetGPGList'
import { usePostGPGKey } from '@/hooks/usePostGPGKey'
import { legacyApiClient } from '@/utils/queryClient'

const GpgKeyItem = ({ keyData }: { keyData: GpgKey }) => {
  const { mutate: deleteGPGKey } = useDeleteGPGKeyById()
  const fetchGPGList = legacyApiClient.v1.getApiGpgList()
  const queryClient = useQueryClient()

  return (
    <div className='border-primary flex items-center justify-between border-b py-4 last:border-b-0'>
      <div className='flex items-start'>
        <LockIcon className='text-quaternary h-6 w-6' aria-hidden='true' />
        <div className='ml-4'>
          <p className='text-primary text-base font-bold'>{keyData.fingerprint}</p>
          <p className='text-tertiary mt-1 font-mono text-sm'>{keyData.fingerprint}</p>
          <p className='text-tertiary mt-2 text-xs'>
            <HandleTime created_at={Math.floor(new Date(keyData.created_at).getTime())} />
          </p>
        </div>
      </div>
      <button
        onClick={() =>
          deleteGPGKey(
            {
              data: {
                key_id: keyData.key_id
              }
            },
            {
              onSuccess: () => {
                queryClient.invalidateQueries({ queryKey: fetchGPGList.requestKey() })
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

interface NewGPGKeyDialogProps {
  open: boolean
  setOpen: (open: boolean) => void
}

const NewGPGKeyDialog = ({ open, setOpen }: NewGPGKeyDialogProps) => {
  const { mutate: postGPGKey, isPending } = usePostGPGKey()
  // const [title, setTitle] = useState('')
  const [gpg_content, setGpg_content] = useState('')
  const [errors, setErrors] = useState<{ title?: string; gpgKey?: string }>({})
  const [expires_days, setExpiresDays] = useState(new Date())

  const fetchGPGList = legacyApiClient.v1.getApiGpgList()
  const queryClient = useQueryClient()

  const handleSubmit = (e?: React.FormEvent | React.MouseEvent) => {
    if (e) e.preventDefault()
    const nextErrors: { title?: string; gpgKey?: string } = {}

    // if (!title.trim()) nextErrors.title = 'Title is required'
    if (!gpg_content.trim()) nextErrors.gpgKey = 'GPG key is required'
    setErrors(nextErrors)
    if (Object.keys(nextErrors).length > 0) return

    // Normalize Windows-style line endings to Unix-style
    const normalizedGpgContent = gpg_content.replace(/\r\n/g, '\n')

    postGPGKey(
      {
        data: {
          gpg_content: normalizedGpgContent
        }
      },
      {
        onSuccess: () => {
          setOpen(false)
          // setTitle('')
          setGpg_content('')
          setErrors({})

          queryClient.invalidateQueries({ queryKey: fetchGPGList.requestKey() })
        }
      }
    )
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen} visuallyHiddenDescription='Add a new GPG key'>
      <Dialog.Title className='w-full p-4'>Add GPG key</Dialog.Title>
      <Dialog.Content className='w-full max-w-md p-4'>
        {/*<div className='mb-4'>*/}
        {/*  <TextField*/}
        {/*    autoFocus*/}
        {/*    label='title'*/}
        {/*    value={title}*/}
        {/*    onChange={setTitle}*/}
        {/*  />*/}
        {/*  {errors.title && <span className='text-red-500 text-xs'>{errors.title}</span>}*/}
        {/*</div>*/}

        <div className='flex h-full w-full flex-col gap-3'>
          <TextField label='expires_days' value={expires_days.toISOString()} disabled />
          <div className='w-full items-center justify-center'>
            <DateAndTimePicker value={expires_days} onChange={setExpiresDays} />
          </div>
        </div>

        <div className='mb-4'>
          <TextField
            placeholder='begins with "-----BEGIN GPG PUBLIC KEY BLOCK-----"'
            multiline
            minRows={8}
            label='gpg_key'
            value={gpg_content}
            onChange={setGpg_content}
          />
          {errors.gpgKey && <span className='text-xs text-red-500'>{errors.gpgKey}</span>}
        </div>
      </Dialog.Content>

      <Dialog.Footer>
        <Dialog.TrailingActions>
          <Button
            variant='flat'
            onClick={() => {
              setOpen(false)
              setGpg_content('')
              setExpiresDays(new Date())
            }}
          >
            Cancel
          </Button>
          <Button
            variant='primary'
            className='bg-[#1f883d]'
            onClick={handleSubmit}
            disabled={isPending || !gpg_content.trim()}
            loading={isPending}
          >
            Add key
          </Button>
        </Dialog.TrailingActions>
      </Dialog.Footer>
    </Dialog.Root>
  )
}

const GPGKeys = () => {
  const { gpgKeys, isLoading: isGPGLoading } = useGetGPGList()
  const [open, setOpen] = useState(false)

  return (
    <>
      <div className='border-primary bg-primary dark:bg-tertiary text-secondary mx-auto max-w-4xl rounded-lg border p-8 font-sans'>
        <header className='flex items-center justify-between pb-4'>
          <h1 className='text-primary text-3xl font-bold'>GPG keys</h1>
          <Button variant='primary' className='bg-[#1f883d]' leftSlot={<PlusIcon />} onClick={() => setOpen(true)}>
            New GPG key
          </Button>
        </header>

        <p className='mb-8'>
          This is a list of GPG keys associated with your account. Remove any keys that you do not recognize.
        </p>

        <section>
          <h2 className='border-primary text-primary border-b pb-2 text-xl font-semibold'>Authentication keys</h2>
          {isGPGLoading ? (
            <div className='flex h-[400px] items-center justify-center'>
              <LoadingSpinner />
            </div>
          ) : (
            <div>
              {gpgKeys?.map((key) => (
                <GpgKeyItem key={key.key_id} keyData={key} />
              ))}
            </div>
          )}
        </section>
      </div>
      <NewGPGKeyDialog open={open} setOpen={setOpen} />
    </>
  )
}

export default GPGKeys
