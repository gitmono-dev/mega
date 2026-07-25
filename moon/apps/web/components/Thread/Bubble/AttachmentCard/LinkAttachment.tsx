import { useEffect, useState } from 'react'
import { useInView } from 'react-intersection-observer'

import { Attachment } from '@gitmono/types'
import { cn } from '@gitmono/ui/src/utils'

import { embedType, embedTypeTrusted, transformUrl } from '@/components/Post/PostEmbeds/transformUrl'
import { stableId } from '@/components/Post/PostReorderableAttachments'

interface Props {
  attachment: Attachment
  selfSize: boolean
}

export function LinkAttachment({ attachment, selfSize }: Props) {
  const [hasLoaded, setHasLoaded] = useState(false)
  const { ref, inView } = useInView()

  useEffect(() => {
    if (!hasLoaded && inView) {
      setHasLoaded(true)
    }
  }, [inView, hasLoaded])

  const url = attachment.url

  if (!url) return null
  const linkType = embedType(url)
  const trusted = embedTypeTrusted(linkType)
  const { src } = transformUrl(linkType, url)

  return (
    <div className={cn('h-full w-full')}>
      <div
        className={cn('flex aspect-video h-full w-full flex-col items-center justify-center', {
          'overflow-hidden rounded-t-[7px]': !selfSize
        })}
      >
        <div ref={ref} className='relative h-full w-full overflow-hidden'>
          {hasLoaded && (
            <iframe
              key={stableId(attachment)}
              src={src}
              allowFullScreen
              className={cn('bg-primary absolute inset-0 h-full w-full object-contain', {
                'overflow-hidden rounded-t-md': !selfSize
              })}
              sandbox={trusted ? undefined : 'allow-scripts allow-same-origin'}
            />
          )}
        </div>
      </div>
    </div>
  )
}
