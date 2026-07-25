import { useEffect } from 'react'
import { ProfileDisplay } from 'components/UserSettings/ProfileDisplay'
import { ProfileSecurity } from 'components/UserSettings/ProfileSecurity'
import { TwoFactorAuthentication } from 'components/UserSettings/TwoFactorAuthentication'
import Head from 'next/head'

import { AdminGroups } from '@/components/AdminGroups'
import { CopyCurrentUrl } from '@/components/CopyCurrentUrl'
import AuthAppProviders from '@/components/Providers/AuthAppProviders'
import ClaStatusSection from '@/components/Setting/ClaStatusSection'
import GPGKeys from '@/components/Setting/GPGKeys'
import PersonalToken from '@/components/Setting/PersonalToken'
import SSHKeys from '@/components/Setting/SSHKeys'
import { MenuPicker } from '@/components/Sidebar/SidebarMenu/MenuPicker'
import { ThemePicker } from '@/components/Theme/ThemePicker'
import { Behaviors } from '@/components/UserSettings/Behaviors'
import { NotificationSettings } from '@/components/UserSettings/Notifications/NotificationSettings'
import { PushNotificationSettings } from '@/components/UserSettings/Notifications/PushNotificationSettings'
import { NotificationSchedule } from '@/components/UserSettings/NotificationSchedule'
import { UserSettingsPageWrapper } from '@/components/UserSettings/PageWrapper'
import { SlackNotificationSettings } from '@/components/UserSettings/SlackNotificationSettings'
import { Timezone } from '@/components/UserSettings/Timezone'
import { useAdminCheck } from '@/hooks/admin/useAdminCheck'
import { PageWithProviders } from '@/utils/types'

const UserSettingsPage: PageWithProviders<any> = () => {
  const { data: adminCheck } = useAdminCheck()
  const isAdmin = adminCheck?.data?.is_admin || false

  useEffect(() => {
    const hash = window.location.hash

    if (!hash) return

    const element = document.querySelector(hash)

    if (element) {
      element.scrollIntoView({ behavior: 'auto' })
    }
  }, [])

  return (
    <>
      <Head>
        <title>Account settings</title>
      </Head>

      <CopyCurrentUrl />

      <UserSettingsPageWrapper>
        <ProfileDisplay />
        <Timezone />
        <SSHKeys />
        <GPGKeys />
        <PersonalToken />
        <ClaStatusSection />
        <ThemePicker />
        {isAdmin && <MenuPicker />}
        <Behaviors />
        <PushNotificationSettings />
        <NotificationSettings />
        <NotificationSchedule />
        <SlackNotificationSettings />
        <ProfileSecurity />
        <TwoFactorAuthentication />
        {isAdmin && <AdminGroups />}
      </UserSettingsPageWrapper>
    </>
  )
}

UserSettingsPage.getProviders = (page, pageProps) => {
  return <AuthAppProviders {...pageProps}>{page}</AuthAppProviders>
}

export default UserSettingsPage
