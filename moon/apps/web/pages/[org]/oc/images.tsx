'use client'

import { useState } from 'react'
import Head from 'next/head'

import { Button, UIText } from '@gitmono/ui'

import { AppLayout } from '@/components/Layout/AppLayout'
import { OrionImagesTable } from '@/components/OrionClient/OrionImagesTable'
import { OrionClientPageWrapper } from '@/components/OrionClient/PageWrapper'
import { UploadOrionImageDialog } from '@/components/OrionClient/UploadOrionImageDialog'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useGetOrionImages } from '@/hooks/OrionClient/useGetOrionImages'
import { PageWithLayout } from '@/utils/types'

const OrionImagesPage: PageWithLayout<any> = () => {
  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false
  const { data: orionImages = [], isLoading: isLoadingImages, error: orionImagesError } = useGetOrionImages(isAdmin)
  const [uploadOpen, setUploadOpen] = useState(false)

  return (
    <>
      <Head>
        <title>Image Management</title>
      </Head>
      <OrionClientPageWrapper>
        <div className='flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4'>
          <div className='flex flex-wrap items-start justify-between gap-3'>
            <div>
              <h1 className='text-xl font-semibold'>Image Management</h1>
              <UIText size='text-sm' color='text-muted' className='mt-1'>
                Browse, upload, and delete Orion VM catalog images.
              </UIText>
            </div>
            {isAdmin ? (
              <Button variant='primary' onClick={() => setUploadOpen(true)}>
                Upload image
              </Button>
            ) : null}
          </div>
          {isAdmin ? (
            <>
              <OrionImagesTable
                images={orionImages}
                isLoading={isLoadingImages}
                error={orionImagesError instanceof Error ? orionImagesError : null}
              />
              <UploadOrionImageDialog open={uploadOpen} onOpenChange={setUploadOpen} />
            </>
          ) : (
            <UIText size='text-sm' color='text-muted'>
              Admin access is required to manage VM images.
            </UIText>
          )}
        </div>
      </OrionClientPageWrapper>
    </>
  )
}

OrionImagesPage.getProviders = (page: React.ReactElement, pageProps: any) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default OrionImagesPage
