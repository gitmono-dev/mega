import { useSetAtom } from 'jotai'
import { useRouter } from 'next/router'

import { Project } from '@gitmono/types'
import { Button } from '@gitmono/ui/Button'
import { LayeredHotkeys } from '@gitmono/ui/DismissibleLayer'
import { LockIcon } from '@gitmono/ui/Icons'
import { cn } from '@gitmono/ui/utils'

import { FloatingNewDocButton } from '@/components/FloatingButtons/NewDoc'
import { FloatingNewPostButton } from '@/components/FloatingButtons/NewPost'
import { IndexPageContainer } from '@/components/IndexPages/components'
import { ProjectArchiveBanner } from '@/components/Projects/ProjectArchiveBanner'
import { ProjectChat } from '@/components/Projects/ProjectChat'
import { ProjectFavoriteButton } from '@/components/Projects/ProjectFavoriteButton'
import { ProjectNotes } from '@/components/Projects/ProjectNotes'
import { ProjectOverflowMenu } from '@/components/Projects/ProjectOverflowMenu'
import { ProjectPosts } from '@/components/Projects/ProjectPosts'
import { ProjectMobileSidebar } from '@/components/Projects/ProjectSidebar'
import { ProjectSidebarDesktopToggleButton } from '@/components/Projects/ProjectSidebar/ProjectSidebarToggleButton'
import { ProjectSplitView } from '@/components/Projects/ProjectSplitView'
import { isDesktopProjectSidebarOpenAtom } from '@/components/Projects/utils'
import { SplitViewContainer } from '@/components/SplitView'
import { useIsSplitViewVisible } from '@/components/SplitView/hooks'
import { ProjectAccessoryBreadcrumbIcon } from '@/components/Titlebar/BreadcrumbPageIcons'
import { BreadcrumbLabel, BreadcrumbTitlebar } from '@/components/Titlebar/BreadcrumbTitlebar'
import { useScope } from '@/contexts/scope'

interface ProjectViewProps {
  project: Project
}

export function ProjectView({ project }: ProjectViewProps) {
  const router = useRouter()
  const { scope } = useScope()
  const { isSplitViewVisible } = useIsSplitViewVisible()
  const setIsDesktopProjectSidebarOpen = useSetAtom(isDesktopProjectSidebarOpenAtom)

  const isChatProject = !!project.message_thread_id
  const isIndex = router.pathname === '/[org]/projects/[projectId]'
  const isPosts = isIndex && !isChatProject
  const isChat = isIndex && isChatProject
  const isDocs = router.pathname === '/[org]/projects/[projectId]/docs'

  function toggleProjectDetailsSidebar() {
    setIsDesktopProjectSidebarOpen((prev) => !prev)
  }

  return (
    <>
      <LayeredHotkeys keys='BracketRight' callback={toggleProjectDetailsSidebar} />
      <LayeredHotkeys keys='1' callback={() => router.push(`/${scope}/projects/${project.id}`)} />
      <LayeredHotkeys
        keys='2'
        callback={() => router.push(`/${scope}/projects/${project.id}/docs`)}
        options={{ enabled: !isChatProject }}
      />
      {isPosts && <FloatingNewPostButton />}
      {isDocs && <FloatingNewDocButton />}

      <SplitViewContainer>
        <IndexPageContainer>
          <BreadcrumbTitlebar>
            <span className='flex items-center gap-1'>
              <ProjectAccessoryBreadcrumbIcon project={project} />
              <BreadcrumbLabel>{project.name}</BreadcrumbLabel>
              {project.private && <LockIcon />}
            </span>
            <ProjectFavoriteButton project={project} shortcutEnabled />
            <ProjectIndexLayoutFilter project={project} />

            <div className={cn('ml-auto flex items-center gap-1.5', { hidden: isSplitViewVisible })}>
              <ProjectOverflowMenu type='dropdown' project={project} size='sm' />
              <ProjectSidebarDesktopToggleButton />
            </div>
          </BreadcrumbTitlebar>

          <MobileTitleBar project={project} />

          <div className='flex flex-1 overflow-hidden'>
            <div className='flex flex-1 flex-col'>
              {isPosts && <ProjectPosts project={project} />}
              {isDocs && <ProjectNotes project={project} />}
              {isChat && <ProjectChat project={project} />}
            </div>
          </div>
        </IndexPageContainer>

        <ProjectSplitView project={project} />
        <ProjectMobileSidebar project={project} />
      </SplitViewContainer>
    </>
  )
}

function ProjectIndexLayoutFilter({ project, fullWidth }: ProjectViewProps & { fullWidth?: boolean }) {
  const router = useRouter()
  const { scope } = useScope()
  const isChatProject = !!project.message_thread_id
  const isRoot = router.pathname === '/[org]/projects/[projectId]'
  const isDocs = router.pathname === '/[org]/projects/[projectId]/docs'

  return (
    <div className='ml-2 flex flex-1 items-center gap-0.5'>
      <Button
        size='sm'
        fullWidth={fullWidth}
        tooltip={isChatProject ? 'Chat' : 'Posts'}
        tooltipShortcut='1'
        href={`/${scope}/projects/${project.id}`}
        variant={isRoot ? 'flat' : 'plain'}
      >
        {isChatProject ? 'Chat' : 'Posts'}
      </Button>
      {!isChatProject && (
        <Button
          size='sm'
          fullWidth={fullWidth}
          tooltip='Docs'
          tooltipShortcut='2'
          href={`/${scope}/projects/${project.id}/docs`}
          variant={isDocs ? 'flat' : 'plain'}
        >
          Docs
        </Button>
      )}
    </div>
  )
}

function MobileTitleBar({ project }: ProjectViewProps) {
  return (
    <BreadcrumbTitlebar hideSidebarToggle className='flex h-auto py-1.5 lg:hidden'>
      <ProjectIndexLayoutFilter project={project} fullWidth />
      {project.archived && <ProjectArchiveBanner />}
    </BreadcrumbTitlebar>
  )
}
