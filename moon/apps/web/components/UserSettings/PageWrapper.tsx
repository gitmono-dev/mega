import { useRouter } from 'next/router'

import { Avatar, UIText } from '@gitmono/ui'

import { PendingAccountReviewButton } from '@/components/AccountReview/PendingAccountReviewButton'
import { BackButton } from '@/components/BackButton'
import { ProfileDropdown } from '@/components/NavigationSidebar/ProfileDropdown'
import { BasicTitlebar } from '@/components/Titlebar'
import { SubnavigationTab } from '@/components/Titlebar/Subnavigation'
import { useScope } from '@/contexts/scope'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { usePendingAccountReviewCount } from '@/hooks/usePendingAccountReviewCount'

interface Props {
  children: React.ReactNode
}

export function UserSettingsPageWrapper(props: Props) {
  const { children } = props
  const router = useRouter()
  const { scope } = useScope()
  const { data: currentUser } = useGetCurrentUser()
  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false
  const { pendingCount } = usePendingAccountReviewCount()
  // Prefer org home over `/` — bare `/` SSR-redirects and feels like a redirect loop from settings
  const backFallback = typeof scope === 'string' && scope.length > 0 ? `/${scope}` : undefined

  return (
    <>
      <div className='bg-primary sticky top-0 z-10'>
        <BasicTitlebar
          className='-mb-2 border-b-0'
          leadingSlot={<BackButton fallbackPath={backFallback} />}
          centerSlot={
            <div className='flex items-center gap-3'>
              <Avatar urls={currentUser?.avatar_urls} name={currentUser?.display_name} size='sm' />
              <UIText weight='font-semibold'>Account settings</UIText>
            </div>
          }
          trailingSlot={
            <div className='flex items-center gap-1'>
              <PendingAccountReviewButton />
              <ProfileDropdown align='end' side='bottom' />
            </div>
          }
        />

        <div className='flex w-full border-b px-4 lg:px-0'>
          <div className='mx-auto flex w-full max-w-3xl items-center justify-center gap-4'>
            <SubnavigationTab href='/me/settings' active={router.pathname === '/me/settings'} replace>
              General
            </SubnavigationTab>
            <SubnavigationTab
              replace
              href='/me/settings/organizations'
              active={router.pathname === '/me/settings/organizations'}
            >
              Organizations
            </SubnavigationTab>
            {isAdmin && (
              <SubnavigationTab
                replace
                href='/me/settings/account-review'
                active={router.pathname === '/me/settings/account-review'}
              >
                Account review
                {pendingCount > 0 && (
                  <span className='h-4.5 min-w-4.5 ml-1 inline-flex items-center justify-center rounded-full bg-amber-500 px-1.5 font-mono text-[10px] font-semibold leading-none text-white'>
                    {pendingCount}
                  </span>
                )}
              </SubnavigationTab>
            )}
          </div>
        </div>
      </div>

      <div className='h-screen overflow-auto'>
        <div className='mx-auto flex w-full max-w-3xl flex-1 flex-col gap-8 px-4 pb-32 pt-8 lg:px-0'>{children}</div>
      </div>
    </>
  )
}
