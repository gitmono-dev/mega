import React, { forwardRef } from 'react'

import { LockIcon } from '@gitmono/ui/Icons'
import { Link } from '@gitmono/ui/Link'
import { UIText } from '@gitmono/ui/Text'
import { cn } from '@gitmono/ui/utils'

import { ProjectHovercard } from '@/components/InlinePost/ProjectHovercard'
import { useScope } from '@/contexts/scope'

type ProjectTagElement = React.ElementRef<typeof Link>
interface ProjectTagProps extends Omit<React.ComponentPropsWithoutRef<typeof Link>, 'href'> {
  project: {
    id: string
    name: string
    private: boolean
    accessory?: string | null
  }
}

export const ProjectTag = forwardRef<ProjectTagElement, ProjectTagProps>(({ project, className, ...props }, ref) => {
  const { scope } = useScope()

  return (
    <ProjectHovercard projectId={project.id} side='top' align='center'>
      <Link
        {...props}
        ref={ref}
        className={cn(
          'hover:text-primary text-quaternary @xl:text-tertiary relative flex items-center gap-1 rounded-full @xl:h-6 @xl:border',
          {
            '@xl:px-2': !project.accessory && !project.private,
            '@xl:pr-2.5 @xl:pl-1.5': project.accessory && !project.private,
            '@xl:pr-1.5 @xl:pl-2.5': project.private && !project.accessory,
            '@xl:px-1.5': project.private && project.accessory
          },
          className
        )}
        href={`/${scope}/projects/${project.id}`}
      >
        {project.accessory && (
          <UIText className='mr-px font-["emoji"] text-xs leading-none'>{project.accessory}</UIText>
        )}
        <UIText className='flex-none' size='text-sm @xl:text-xs' inherit>
          {project.name}
        </UIText>
        {project.private && <LockIcon size={14} className='opacity-80' />}
      </Link>
    </ProjectHovercard>
  )
})
ProjectTag.displayName = 'ProjectTag'
