'use client'

import { useRouter } from 'next/router'

import { SubnavigationTab } from '@/components/Titlebar/Subnavigation'
import { useScope } from '@/contexts/scope'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'

interface Props {
  children: React.ReactNode
}

export function OrionClientPageWrapper({ children }: Props) {
  const router = useRouter()
  const { scope } = useScope()
  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false

  return (
    <div className='flex h-full min-h-0 flex-col'>
      <div className='flex w-full shrink-0 border-b px-4'>
        <div className='flex items-center gap-4'>
          <SubnavigationTab href={`/${scope}/oc`} active={router.pathname === '/[org]/oc'} replace>
            Runners
          </SubnavigationTab>
          {isAdmin ? (
            <SubnavigationTab
              href={`/${scope}/oc/images`}
              active={router.pathname.startsWith('/[org]/oc/images')}
              replace
            >
              Image Management
            </SubnavigationTab>
          ) : null}
        </div>
      </div>
      {children}
    </div>
  )
}
