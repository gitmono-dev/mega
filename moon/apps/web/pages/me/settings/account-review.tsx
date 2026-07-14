import { useEffect } from 'react'
import Head from 'next/head'
import { useRouter } from 'next/router'

import { CopyCurrentUrl } from '@/components/CopyCurrentUrl'
import { FullPageError } from '@/components/Error'
import { FullPageLoading } from '@/components/FullPageLoading'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import { AccountReviewSection } from '@/components/UserSettings/AccountReview/AccountReviewSection'
import { UserSettingsPageWrapper } from '@/components/UserSettings/PageWrapper'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { PageWithProviders } from '@/utils/types'

const AccountReviewPage: PageWithProviders<any> = () => {
  const router = useRouter()
  const { data: adminCheck, isPending, isError, isFetched } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin === true

  useEffect(() => {
    // Only bounce when we know the user is not an admin — not on API/CORS errors
    if (isFetched && !isError && !isAdmin) {
      router.replace('/me/settings')
    }
  }, [isAdmin, isError, isFetched, router])

  if (isPending) {
    return <FullPageLoading />
  }

  if (isError) {
    return <FullPageError message='Unable to verify admin access. Check Mono API connectivity (CORS).' />
  }

  if (!isAdmin) {
    return <FullPageLoading />
  }

  return (
    <>
      <Head>
        <title>Account review</title>
      </Head>

      <CopyCurrentUrl />

      <UserSettingsPageWrapper>
        <AccountReviewSection />
      </UserSettingsPageWrapper>
    </>
  )
}

AccountReviewPage.getProviders = (page, pageProps) => {
  return <AuthAppProviders {...pageProps}>{page}</AuthAppProviders>
}

export default AccountReviewPage
