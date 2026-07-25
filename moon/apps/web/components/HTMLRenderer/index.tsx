import { useEffect, useId } from 'react'

import { specialLinkClickHandler } from '@gitmono/ui/Link'
import { cn } from '@gitmono/ui/src/utils'

import { useScope } from '@/contexts/scope'
import { useReadOnlyOnCheckboxClick } from '@/hooks/useReadOnlyOnCheckboxClick'

interface Props {
  text?: string
  linkOptions?: {
    truncate?: boolean
  }
  blur?: boolean
  onCheckboxClick?: ({ index, checked }: { index: number; checked: boolean }) => void
  className?: string
  as?: 'div' | 'span'
}

export function HTMLRenderer({
  text,
  onCheckboxClick,
  linkOptions,
  blur = false,
  className = 'prose w-full max-w-full select-text focus:outline-hidden',
  as = 'div',
  ...props
}: Props) {
  const containerId = useId()
  const { scope } = useScope()
  const Element = as

  useReadOnlyOnCheckboxClick({ containerId, onCheckboxClick })

  // intercept clicks on internal links to keep navigation in-app
  useEffect(() => {
    const container = document.getElementById(containerId)

    if (!container) return

    const listener = (e: MouseEvent) => {
      specialLinkClickHandler(`${scope}`, e)
    }

    const links = container.querySelectorAll('a')

    links.forEach((link) => {
      link.addEventListener('click', listener)
    })

    return () => {
      links.forEach((link) => {
        link.removeEventListener('click', listener)
      })
    }
  }, [containerId, scope])

  return (
    <Element
      {...props}
      id={containerId}
      className={cn(className, {
        'pointer-events-none blur filter will-change-transform select-none': blur,
        'truncate-links': !!linkOptions?.truncate
      })}
      dangerouslySetInnerHTML={{ __html: text ?? '' }}
    />
  )
}
