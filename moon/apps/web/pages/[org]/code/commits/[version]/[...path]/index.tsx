import React from 'react'

import { cn } from '@gitmono/ui'

import { CommitsView } from '@/components/CodeView/CommitsView'
import { IndexPageContainer, IndexPageContent } from '@/components/IndexPages/components'
import { AppLayout } from '@/components/Layout/AppLayout'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { IssueBreadcrumbIcon } from '@/components/Titlebar/BreadcrumbPageIcons'
import { BreadcrumbTitlebar } from '@/components/Titlebar/BreadcrumbTitlebar'

function CommitsPage() {
  return (
    <>
      <IndexPageContainer>
        <BreadcrumbTitlebar>
          <IssueBreadcrumbIcon />
        </BreadcrumbTitlebar>

        <IndexPageContent
          id='/[org]/commits'
          className={cn('@container', 'max-w-full lg:max-w-5xl xl:max-w-6xl 2xl:max-w-7xl')}
        >
          <CommitsView />
        </IndexPageContent>
      </IndexPageContainer>
    </>
  )
}

CommitsPage.getProviders = (
  page: React.ReactNode,
  pageProps: React.JSX.IntrinsicAttributes & { children?: React.ReactNode }
) => {
  return (
    <AuthAppProviders {...pageProps}>
      <AppLayout {...pageProps}>{page}</AppLayout>
    </AuthAppProviders>
  )
}

export default CommitsPage
