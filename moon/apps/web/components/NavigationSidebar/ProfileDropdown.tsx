import React, { forwardRef, useState } from 'react'
import { useSignoutUser } from 'hooks/useSignoutUser'
import * as R from 'remeda'

// import { isMobile } from 'react-device-detect'
// import { SITE_URL } from '@gitmono/config'
// import { useIsDesktopApp } from '@gitmono/ui/src/hooks'

import {
  Badge,
  Button,
  DotsHorizontal,
  GearIcon,
  Link,
  LogOutIcon,
  UserCircleIcon,
  UserCirclePlusIcon
  // HelpIcon,
  // LinearIcon,
  // MonitorIcon,
  // ZapierIcon,
  // FigmaOutlineIcon,
  // CalendarIcon,
  // CodeIcon,
  // AccessIcon,
  // AppsIcon
} from '@gitmono/ui'
import { DropdownMenu } from '@gitmono/ui/DropdownMenu'
import { buildMenuItems } from '@gitmono/ui/Menu'

import { NotificationPauseCalendarDialog } from '@/components/NotificationPause/NotificationPauseCalendarDialog'
import { NotificationScheduleDialog } from '@/components/NotificationPause/NotificationScheduleDialog'
import { useScope } from '@/contexts/scope'
import { useCurrentUserIsStaff } from '@/hooks/useCurrentUserIsStaff'
import { useGetCurrentUser } from '@/hooks/useGetCurrentUser'
import { usePauseNotificationMenuItem } from '@/hooks/usePauseNotificationMenuItem'
import { usePendingAccountReviewCount } from '@/hooks/usePendingAccountReviewCount'

interface LinkProps {
  children: React.ReactNode
  href: string
  [key: string]: any
}

const SettingsLink = forwardRef((props: LinkProps, ref: React.ForwardedRef<HTMLAnchorElement>) => {
  let { href, children, ...rest } = props

  return (
    <Link href={href} ref={ref} {...rest}>
      {children}
    </Link>
  )
})

SettingsLink.displayName = 'SettingsLink'

export function ProfileDropdown({
  trigger,
  side = 'top',
  align = 'start'
}: {
  trigger?: React.ReactNode
  side?: 'top' | 'bottom'
  align?: 'start' | 'end' | 'center'
}) {
  const { scope } = useScope()
  const { data: currentUser } = useGetCurrentUser()
  const signout = useSignoutUser()
  // const isDesktop = useIsDesktopApp()
  const isStaff = useCurrentUserIsStaff()
  const { isAdmin, pendingCount, hasPending } = usePendingAccountReviewCount()
  const [open, setOpen] = useState(false)
  const [notificationPauseCalendarDialogOpen, setNotificationPauseCalendarDialogOpen] = useState(false)
  const [notificationScheduleDialogOpen, setNotificationScheduleDialogOpen] = useState(false)
  const pauseNotificationsMenuItem = usePauseNotificationMenuItem({
    setNotificationPauseCalendarDialogOpen,
    setNotificationScheduleDialogOpen
  })

  const topBarItems = buildMenuItems([
    {
      type: 'item',
      label: 'My profile',
      leftSlot: <UserCircleIcon />,
      external: false,
      url: `/${scope}/people/${currentUser?.username}`,
      onSelect: () => setOpen(false)
    },
    {
      type: 'item',
      label: 'Account settings',
      leftSlot: <GearIcon />,
      external: false,
      url: '/me/settings'
    },
    isAdmin && {
      type: 'item',
      label: 'Account review',
      leftSlot: <UserCirclePlusIcon />,
      external: false,
      url: '/me/settings/account-review',
      onSelect: () => setOpen(false),
      rightSlot: hasPending ? (
        <Badge color='amber' className='h-4.5 ml-1 flex font-mono'>
          {pendingCount}
        </Badge>
      ) : undefined
    },
    pauseNotificationsMenuItem,
    { type: 'separator' },
    // {
    //   type: 'item',
    //   label: 'Support',
    //   leftSlot: <HelpIcon />,
    //   url: `mailto:support@gitmono.com`
    // },
    // { type: 'separator' },
    // !isMobile && {
    //   type: 'sub',
    //   label: 'Apps & integrations',
    //   leftSlot: <AppsIcon />,
    //   items: buildMenuItems([
    //     !isDesktop && {
    //       type: 'item',
    //       label: 'Desktop app',
    //       leftSlot: <MonitorIcon />,
    //       external: true,
    //       url: `${SITE_URL}/desktop/download`
    //     },
    //     {
    //       type: 'item',
    //       label: 'Linear',
    //       leftSlot: <LinearIcon />,
    //       external: true,
    //       url: 'https://linear.app/integrations/campsite'
    //     },
    //     {
    //       type: 'item',
    //       label: 'Zapier',
    //       leftSlot: <ZapierIcon />,
    //       external: true,
    //       url: 'https://zapier.com/apps/campsite/integrations'
    //     },
    //     {
    //       type: 'item',
    //       label: 'Figma',
    //       leftSlot: <FigmaOutlineIcon />,
    //       external: true,
    //       url: `${SITE_URL}/figma/plugin`
    //     },
    //     {
    //       type: 'item',
    //       label: 'Cal.com',
    //       leftSlot: <CalendarIcon />,
    //       external: true,
    //       url: 'https://app.cal.com/apps/campsite'
    //     },
    //     {
    //       type: 'item',
    //       label: 'API',
    //       leftSlot: <CodeIcon />,
    //       external: true,
    //       url: 'https://developers.campsite.com'
    //     }
    //   ])
    // },
    // !isMobile && { type: 'separator' },
    // isStaff && {
    //   type: 'item',
    //   label: 'Feature flags',
    //   leftSlot: <AccessIcon />,
    //   external: true,
    //   url:
    //     !process.env.NODE_ENV || process.env.NODE_ENV === 'development'
    //       ? 'http://admin.gitmega.com/admin/features/'
    //       : 'https://admin.campsite.com/admin/features'
    // },
    isStaff && { type: 'separator' },
    {
      type: 'item',
      label: 'Sign out',
      leftSlot: <LogOutIcon />,
      onSelect: () => signout.mutate(),
      destructive: true
    }
  ])

  const items = R.filter(topBarItems, R.isTruthy)

  return (
    <>
      <DropdownMenu
        open={open}
        onOpenChange={setOpen}
        items={items}
        align={align}
        side={side}
        trigger={
          trigger ? (
            <span className='flex'>{trigger}</span>
          ) : (
            <Button variant='plain' iconOnly={<DotsHorizontal />} accessibilityLabel='Account options' />
          )
        }
      />
      <NotificationPauseCalendarDialog
        key={`notification-pause-calendar-dialog-${notificationPauseCalendarDialogOpen}`}
        open={notificationPauseCalendarDialogOpen}
        onOpenChange={setNotificationPauseCalendarDialogOpen}
      />
      <NotificationScheduleDialog
        key={`notification-schedule-dialog-${notificationScheduleDialogOpen}`}
        open={notificationScheduleDialogOpen}
        onOpenChange={setNotificationScheduleDialogOpen}
      />
    </>
  )
}
