import React from 'react'
import Head from 'next/head'
import { useRouter } from 'next/router'

import { Body, Button, Logo, Title2 } from '@gitmono/ui'
import { ToasterProvider } from '@gitmono/ui/Toast'

import { FullPageError } from '@/components/Error'
import { FullPageLoading } from '@/components/FullPageLoading'
import { useMyApprovalStatus } from '@/hooks/AccountReview/useMyApprovalStatus'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { useSignoutUser } from '@/hooks/useSignoutUser'

interface Props {
  children: React.ReactNode
  allowLoggedOut: boolean
}

const AccountApprovalGuard: React.FC<Props> = ({ children, allowLoggedOut }) => {
  const router = useRouter()
  const getCurrentUser = useGetCurrentUser()
  const { data: adminCheck, isPending: adminPending, isError: adminError } = useAdminCheck()
  const signout = useSignoutUser()
  const currentUser = getCurrentUser.data

  const isAdmin = adminCheck?.data?.is_admin === true
  const loggedIn = !!currentUser?.logged_in
  // Account settings must stay reachable (admin review tab, back navigation). Only gate org/home.
  const isAccountRoute = router.pathname.startsWith('/me/')

  // Admin-check failure (e.g. missing `.mega_cedar.json`) is treated as non-admin:
  // still require / fall through to user_approval_status.
  const adminCheckDone = !adminPending || adminError
  const needsApprovalCheck = !allowLoggedOut && loggedIn && !isAccountRoute && !isAdmin && adminCheckDone
  const {
    data: approvalRes,
    isPending: approvalPending,
    isError: approvalError,
    isFetched: approvalFetched
  } = useMyApprovalStatus(needsApprovalCheck)

  if (allowLoggedOut) {
    return <>{children}</>
  }

  if (getCurrentUser.error) {
    return <FullPageError message='We ran into an issue starting the app' />
  }

  if (!getCurrentUser.data && getCurrentUser.isLoading) {
    return <FullPageLoading />
  }

  // Not logged in — AuthProvider handles redirect; don't block here
  if (!loggedIn) {
    return <>{children}</>
  }

  // Always allow account/settings routes — avoids settings ↔ home redirect thrash
  if (isAccountRoute) {
    return <>{children}</>
  }

  // Wait for admin check unless it already failed — failure ⇒ non-admin + approval check
  if (!adminCheckDone) {
    return <FullPageLoading />
  }

  // Admins do not need account review
  if (isAdmin) {
    return <>{children}</>
  }

  // Do not default to "pending" before approval status has actually loaded.
  if (needsApprovalCheck && (approvalPending || (!approvalFetched && !approvalError))) {
    return <FullPageLoading />
  }

  if (approvalError) {
    return <FullPageError message='Unable to verify account approval status' />
  }

  const status = approvalRes?.data?.status ?? 'pending'

  if (status === 'approved') {
    return <>{children}</>
  }

  const isRejected = status === 'rejected'

  return (
    <>
      <Head>
        <title>{isRejected ? 'Account not approved' : 'Account pending review'}</title>
      </Head>

      <ToasterProvider />

      <div className='bg-secondary flex flex-1 flex-col items-center justify-center gap-8 p-4'>
        <Logo />
        <div className='flex w-full max-w-md flex-col rounded-md text-center'>
          <Title2>{isRejected ? 'Account not approved' : 'Account pending review'}</Title2>

          <Body className='mt-4' secondary>
            {isRejected
              ? 'Your account was not approved by an admin, so you cannot access the home page. Contact an admin if you have questions.'
              : 'Your account is waiting for admin review. You can access the home page once it is approved.'}
          </Body>

          <div className='mt-6 space-y-6'>
            <Button fullWidth variant='plain' onClick={() => signout.mutate()}>
              Sign out
            </Button>
          </div>
        </div>
      </div>
    </>
  )
}

export default AccountApprovalGuard
