import React, { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import toast from 'react-hot-toast'

import { ListToken } from '@gitmono/types'
import { Button, LoadingSpinner, LockIcon, PlusIcon } from '@gitmono/ui'

import HandleTime from '@/components/ClView/components/HandleTime'
import { useDeleteTokenById } from '@/hooks/useDeleteTokenById'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useGetTokenList } from '@/hooks/useGetTokenList'
import { usePostTokenGenerate } from '@/hooks/usePostTokenGenerate'
import { legacyApiClient } from '@/utils/queryClient'

const TokenItem = ({ item }: { item: ListToken }) => {
  const { mutate: deleteToken } = useDeleteTokenById()
  const queryClient = useQueryClient()
  const fetchTokenList = legacyApiClient.v1.getApiUserTokenList()

  return (
    <div className='border-primary flex items-center justify-between border-b py-4 last:border-b-0'>
      <div className='flex items-start'>
        <LockIcon className='text-quaternary h-6 w-6' aria-hidden='true' />
        <div className='ml-4'>
          <p className='text-primary text-base font-bold'>Token #{item.id}</p>
          <p className='text-tertiary mt-1 font-mono text-sm break-all'>{item.token}</p>
          <p className='text-tertiary mt-2 text-xs'>
            <HandleTime created_at={item.created_at} />
          </p>
        </div>
      </div>
      <button
        onClick={() =>
          deleteToken(
            { keyId: item.id },
            {
              onSuccess: () => {
                queryClient.invalidateQueries({ queryKey: fetchTokenList.requestKey() })
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

const CopySpace = ({ copyText }: { copyText: string }) => {
  const [copied, setCopied] = useState(false)
  const handleCopy = async (copyText: string) => {
    if (navigator.clipboard) {
      await navigator.clipboard
        .writeText(copyText)
        .then(() => toast.success('Copied to clipboard'))
        .catch(() => toast.error('Copied failed'))
    } else {
      const textArea = document.createElement('textarea')

      textArea.value = copyText
      document.body.appendChild(textArea)
      textArea.select()
      try {
        document.execCommand('copy')
        toast.success('Copied to clipboard')
        document.body.removeChild(textArea)
      } catch {
        toast.error('Copied failed')
      }
    }
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className='mb-4 flex items-center gap-3'>
      <code className='border-primary bg-secondary flex-1 rounded border px-3 py-2 font-mono text-sm break-all'>
        {copyText}
      </code>
      <Button variant='flat' onClick={() => handleCopy(copyText)}>
        {copied ? 'Copied' : 'Copy'}
      </Button>
    </div>
  )
}

const PersonalToken = () => {
  const { tokenList, isLoading } = useGetTokenList()
  const { mutate: generateToken, isPending: isGenerating } = usePostTokenGenerate()
  const queryClient = useQueryClient()
  const fetchTokenList = legacyApiClient.v1.getApiUserTokenList()

  const [generated, setGenerated] = useState<string | null>(null)
  const { data: currentUser, isLoading: isUserLoading } = useGetCurrentUser()

  const handleGenerate = () => {
    generateToken(undefined, {
      onSuccess: (result) => {
        setGenerated(result?.data ?? null)
        queryClient.invalidateQueries({ queryKey: fetchTokenList.requestKey() })
      }
    })
  }

  return (
    <div className='border-primary bg-primary dark:bg-tertiary text-secondary mx-auto mt-8 max-w-4xl rounded-lg border p-8 font-sans'>
      <header className='flex items-center justify-between pb-4'>
        <h1 className='text-primary text-3xl font-bold'>Personal tokens</h1>
        <Button
          variant='primary'
          leftSlot={<PlusIcon />}
          onClick={handleGenerate}
          disabled={isGenerating}
          loading={isGenerating}
          className='bg-[#1f883d]'
        >
          New token
        </Button>
      </header>

      <p className='mb-8'>
        This is a list of personal access tokens associated with your account. Remove any tokens that you do not
        recognize.
      </p>

      {generated && currentUser && (
        <div className='mb-8 rounded-md border border-green-200 bg-green-50 p-4 dark:border-green-800 dark:bg-green-950'>
          <p className='text-sm text-green-800 dark:text-green-200'>Your new token has been generated.</p>
          <div className='mt-3 flex flex-col gap-2'>
            <div>
              <span className='text-sm font-medium text-green-700 dark:text-green-300'>Username:</span>
              <CopySpace copyText={currentUser.username} />
            </div>
            <div>
              <span className='text-sm font-medium text-green-700 dark:text-green-300'>Token:</span>
              <CopySpace copyText={generated} />
            </div>
          </div>
          <p className='mt-3 text-xs text-green-600 dark:text-green-400'>
            Make sure to copy your new token now. You won&apos;t be able to see it again.
          </p>
        </div>
      )}

      <section>
        <h2 className='border-primary text-primary border-b pb-2 text-xl font-semibold'>Tokens</h2>
        {isLoading || isUserLoading ? (
          <div className='flex h-[400px] items-center justify-center'>
            <LoadingSpinner />
          </div>
        ) : (
          <div>{currentUser && tokenList.map((item) => <TokenItem key={item.id} item={item} />)}</div>
        )}
      </section>
    </div>
  )
}

export default PersonalToken
