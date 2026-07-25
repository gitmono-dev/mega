import React from 'react'
import { useRouter } from 'next/router'

import { Vec } from '@gitmono/types/generated'

import { SidebarCode } from '@/components/Sidebar/SidebarCode'
import { SidebarDrafts } from '@/components/Sidebar/SidebarDrafts'
import { SidebarInbox } from '@/components/Sidebar/SidebarInbox'
import { SidebarIssue } from '@/components/Sidebar/SidebarIssue'
import { SidebarLink } from '@/components/Sidebar/SidebarLink'
import { SidebarMergeQueue } from '@/components/Sidebar/SidebarMergeQueue'
import { SidebarDocs, SidebarHome, SidebarMessages } from '@/components/Sidebar/SidebarMyWorkGroup'
import { SidebarOc } from '@/components/Sidebar/SidebarOc'
import { SidebarTags } from '@/components/Sidebar/SidebarTags'
import { SidebarChangeList } from '@/components/Sidebar/SiderbarChangeList'
import { useScope } from '@/contexts/scope'

export interface DynamicSidebarItemProps {
  config: Vec[number]
}

const componentMap = {
  home: SidebarHome,
  inbox: SidebarInbox,
  chat: SidebarMessages,
  notes: SidebarDocs,
  drafts: SidebarDrafts,
  code: SidebarCode,
  tags: SidebarTags,
  cl: SidebarChangeList,
  mq: SidebarMergeQueue,
  issue: SidebarIssue,
  oc: SidebarOc
}

export function DynamicSidebarItem({ config }: DynamicSidebarItemProps) {
  const router = useRouter()
  const { scope } = useScope()

  if (!config.visible || config.public_id === 'calls') {
    return null
  }

  const href = `/${scope}${config.href}`
  const isActive = router.pathname === `/[org]${config.href}`

  const Component = componentMap[config.public_id as keyof typeof componentMap]

  if (!Component) {
    return <SidebarLink id={config.public_id} label={config.label} href={href} active={isActive} />
  }

  return <Component label={config.label} href={href} active={isActive} />
}
