import { useState } from 'react'
import Head from 'next/head'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { Button, Dialog, LoadingSpinner, Logo } from '@gitmono/ui'

import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { BasicTitlebar } from '@/components/Titlebar'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useGetClaContent } from '@/hooks/Cla/useGetClaContent'
import { usePostClaChangeSignStatus } from '@/hooks/Cla/usePostClaChangeSignStatus'
import { usePostClaContent } from '@/hooks/Cla/usePostClaContent'
import { PageWithProviders } from '@/utils/types'

const CLASignPage: PageWithProviders<any> = () => {
  const router = useRouter()
  const [agreed, setAgreed] = useState(false)
  const [editDialogOpen, setEditDialogOpen] = useState(false)
  const [editContent, setEditContent] = useState('')

  const { data: claData, isLoading: isContentLoading } = useGetClaContent()
  const { mutate: signCla, isPending: isSigning } = usePostClaChangeSignStatus()
  const { mutate: saveClaContent, isPending: isSaving } = usePostClaContent()
  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false

  const claContent = claData?.data?.content ?? ''

  const handleSign = () => {
    if (!agreed) return
    signCla(undefined, {
      onSuccess: () => {
        toast.success('CLA signed successfully')
        router.back()
      },
      onError: () => {
        toast.error('Failed to sign CLA. Please try again.')
      }
    })
  }

  const handleOpenEdit = () => {
    setEditContent(claContent)
    setEditDialogOpen(true)
  }

  const handleSaveEdit = () => {
    saveClaContent(
      { content: editContent },
      {
        onSuccess: () => {
          toast.success('CLA content updated successfully')

          setEditDialogOpen(false)
        },
        onError: () => {
          toast.error('Failed to update CLA content. Please try again.')
        }
      }
    )
  }

  return (
    <>
      <Head>
        <title>Sign CLA - Contributor License Agreement</title>
      </Head>

      <main className='grid h-full grid-rows-[auto_1fr]'>
        <BasicTitlebar leadingSlot={<Logo />} disableBottomBorder />

        <div className='overflow-auto'>
          <div className='mx-auto max-w-4xl px-6 py-8'>
            <div className='bg-primary border-primary rounded-xl border p-8 shadow-lg'>
              <div className='mt-1 mb-2 flex items-start justify-between'>
                <h1 className='text-primary text-3xl font-bold'>Contributor License Agreement</h1>
                {isAdmin && (
                  <Button variant='plain' onClick={handleOpenEdit}>
                    Edit CLA
                  </Button>
                )}
              </div>
              <p className='text-tertiary mb-6 text-base'>
                Thank you for your interest in contributing to our project. In order to clarify the intellectual
                property license granted with contributions, we need you to sign this Contributor License Agreement
                (CLA).
              </p>

              <div className='bg-secondary border-primary mb-6 max-h-[400px] overflow-y-auto rounded-lg border p-6'>
                {isContentLoading ? (
                  <div className='flex items-center justify-center py-8'>
                    <LoadingSpinner />
                  </div>
                ) : (
                  <div className='text-primary text-sm leading-relaxed whitespace-pre-wrap'>{claContent}</div>
                )}
              </div>

              <div className='mb-6 rounded-lg bg-blue-50 p-4 dark:bg-blue-950/30'>
                <label htmlFor='cla-agreement-checkbox' className='flex cursor-pointer items-start gap-3'>
                  <input
                    id='cla-agreement-checkbox'
                    type='checkbox'
                    checked={agreed}
                    onChange={(e) => setAgreed(e.target.checked)}
                    className='mt-1 h-5 w-5 cursor-pointer rounded border-gray-300 text-blue-600 focus:ring-2 focus:ring-blue-500'
                  />
                  <span className='text-primary text-sm leading-relaxed font-medium'>
                    I have read and agree to the Contributor License Agreement terms and conditions stated above.
                  </span>
                </label>
              </div>

              <div className='flex gap-3'>
                <Button variant='primary' onClick={handleSign} disabled={!agreed || isSigning} fullWidth>
                  {isSigning ? 'Signing...' : 'Sign CLA'}
                </Button>
                <Button variant='plain' onClick={() => router.back()} disabled={isSigning}>
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        </div>
      </main>

      <Dialog.Root
        open={editDialogOpen}
        onOpenChange={setEditDialogOpen}
        size='2xl'
        visuallyHiddenTitle='Edit CLA Content'
      >
        <div className='flex flex-col gap-4 p-6'>
          <h2 className='text-primary text-xl font-semibold'>Edit CLA Content</h2>
          <p className='text-secondary text-sm'>
            Update the Contributor License Agreement text. All users will see the new version.
          </p>
          <textarea
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            rows={16}
            className='border-primary bg-secondary text-primary w-full rounded-lg border p-3 text-sm leading-relaxed focus:ring-2 focus:ring-blue-500 focus:outline-hidden'
            placeholder='Enter CLA content...'
          />
          <div className='flex justify-end gap-3'>
            <Button variant='plain' onClick={() => setEditDialogOpen(false)} disabled={isSaving}>
              Cancel
            </Button>
            <Button variant='primary' onClick={handleSaveEdit} disabled={isSaving || !editContent.trim()}>
              {isSaving ? 'Saving...' : 'Save Changes'}
            </Button>
          </div>
        </div>
      </Dialog.Root>
    </>
  )
}

CLASignPage.getProviders = (page, pageProps) => {
  return <AuthAppProviders {...pageProps}>{page}</AuthAppProviders>
}

export default CLASignPage
