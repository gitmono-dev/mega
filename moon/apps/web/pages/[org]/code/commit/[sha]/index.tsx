import React from 'react'

import CommitsDetailView from '@/components/CodeView/CommitsView/detail'
import { AppLayout } from '@/components/Layout/AppLayout'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'

function CommitsDetailPage() {
  return (
    <>
      <CommitsDetailView />
    </>
  )
}

CommitsDetailPage.getProviders = (
  page: React.ReactNode,
  pageProps: React.JSX.IntrinsicAttributes & { children?: React.ReactNode }
) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default CommitsDetailPage
