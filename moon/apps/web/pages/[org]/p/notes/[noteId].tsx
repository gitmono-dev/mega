import { useEffect, useMemo, useRef, useState } from 'react'
import { QueryClientProvider } from '@tanstack/react-query'
import { domMax, LazyMotion } from 'framer-motion'
import { GetStaticPropsContext } from 'next'
import { generateNextSeo } from 'next-seo/pages'
import dynamic from 'next/dynamic'
import Head from 'next/head'
import { isMacOs } from 'react-device-detect'

import { SITE_URL, WEB_URL } from '@gitmono/config'
import { getNoteExtensions } from '@gitmono/editor'
import { PublicNote } from '@gitmono/types'
import { Button, Link, Logo, UIText } from '@gitmono/ui'
import { useIsDesktopApp } from '@gitmono/ui/src/hooks'
import { cn } from '@gitmono/ui/src/utils'

import { CopyCurrentUrl } from '@/components/CopyCurrentUrl'
import { GlobalMetaTags } from '@/components/Providers/MetaTags'
import { ThemeProvider } from '@/components/Providers/ThemeProvider'
import { ScrollableContainer } from '@/components/ScrollableContainer'
import { ScopeProvider } from '@/contexts/scope'
import { useCreateNoteView } from '@/hooks/useCreateNoteView'
import { QueryNormalizerProvider } from '@/utils/normy/QueryNormalizerProvider'
import { queryClient, ssrApiClient } from '@/utils/queryClient'
import { getNormalizedKey } from '@/utils/queryNormalization'
import { PageWithLayout } from '@/utils/types'

const AttachmentLightbox = dynamic(
  () => import('@/components/AttachmentLightbox').then((mod) => mod.AttachmentLightbox),
  {
    ssr: false
  }
)

const RichTextRenderer = dynamic(() => import('@/components/RichTextRenderer').then((mod) => mod.RichTextRenderer), {
  ssr: false
})

const NotePage: PageWithLayout<any> = ({ note }) => {
  const { mutate: createView } = useCreateNoteView()
  const isDesktopApp = useIsDesktopApp()
  const hasCreatedViewRef = useRef(false)
  const [openAttachmentId, setOpenAttachmentId] = useState<string | undefined>()

  useEffect(() => {
    if (hasCreatedViewRef.current) return
    hasCreatedViewRef.current = true
    createView({ noteId: note.id })
  }, [note.id, createView])

  const extensions = useMemo(() => getNoteExtensions({ linkUnfurl: {} }), [])
  const options = useMemo(() => {
    return {
      mediaGallery: { onOpenAttachment: setOpenAttachmentId },
      postNoteAttachment: { onOpenAttachment: setOpenAttachmentId }
    }
  }, [])

  const UTM_URL = `${SITE_URL}?utm_source=public_note&utm_medium=web&utm_campaign=public_note_share&utm_content=${note.id}`

  return (
    <>
      <CopyCurrentUrl />

      <AttachmentLightbox
        subject={note}
        selectedAttachmentId={openAttachmentId}
        onClose={() => setOpenAttachmentId(undefined)}
        onSelectAttachment={({ id }) => setOpenAttachmentId(id)}
      />

      <nav
        className={cn('drag bg-primary flex items-center justify-between gap-3 border-b py-2.5 pr-2.5 pl-4', {
          'pl-22': isDesktopApp && isMacOs
        })}
      >
        <Link href={UTM_URL}>
          <Logo />
        </Link>
        <Button variant='base' href={UTM_URL}>
          Made with Campsite
        </Button>
      </nav>
      <ScrollableContainer>
        <div className='mx-auto flex w-full max-w-[44rem] flex-1 flex-col gap-4 px-4 pb-4 md:px-6 lg:px-0'>
          <div className='flex flex-col gap-3 py-8 select-text md:py-14 lg:py-20'>
            <UIText
              element='h1'
              className={cn(
                '-mx-px mb-1 w-full border-0 bg-transparent p-0 text-[clamp(2rem,_4vw,_2.5rem)] leading-[1.2] font-bold outline-hidden focus:border-0 focus:ring-0 focus:outline-hidden'
              )}
            >
              {note.title || 'Untitled'}
            </UIText>

            <div className='prose note'>
              <RichTextRenderer content={note.description_html} extensions={extensions} options={options} />
            </div>
          </div>
        </div>
      </ScrollableContainer>
    </>
  )
}

function PublicNoteProviders({ children, note }: { children: React.ReactNode; note: PublicNote }) {
  const [client] = useState(() => queryClient())

  const ogImageUrl = new URL('/api/og', WEB_URL)

  ogImageUrl.searchParams.set('title', note.title)
  ogImageUrl.searchParams.set('org', note.organization.name)
  ogImageUrl.searchParams.set('orgAvatar', note.og_org_avatar)

  return (
    <LazyMotion features={domMax}>
      <QueryNormalizerProvider
        queryClient={client}
        normalizerConfig={{
          getNormalizationObjectKey: getNormalizedKey,
          devLogging: false,
          normalize: true
        }}
      >
        <QueryClientProvider client={client}>
          <ScopeProvider>
            <ThemeProvider>
              <GlobalMetaTags />

              <Head>
                {generateNextSeo({
                  title: note.title,
                  openGraph: {
                    title: note.title,
                    images: [
                      {
                        url: ogImageUrl.toString(),
                        alt: `Feature image for ${note.title}`
                      }
                    ],
                    url: note.url,
                    siteName: 'Campsite',
                    type: 'website',
                    locale: 'en_US'
                  },
                  twitter: {
                    handle: '@trycampsite',
                    cardType: 'summary_large_image',
                    site: '@trycampsite'
                  }
                })}
              </Head>

              {children}
            </ThemeProvider>
          </ScopeProvider>
        </QueryClientProvider>
      </QueryNormalizerProvider>
    </LazyMotion>
  )
}

NotePage.getProviders = (page, { note }) => {
  return <PublicNoteProviders note={note}>{page}</PublicNoteProviders>
}

export default NotePage

export async function getStaticPaths() {
  return {
    paths: [],
    fallback: 'blocking'
  }
}

export async function getStaticProps({ params }: GetStaticPropsContext) {
  const org = params?.org as string
  const noteId = params?.noteId as string

  const parsedNoteId = noteId.includes('-') ? noteId.split('-').pop() : noteId

  if (!org || !parsedNoteId) {
    return {
      notFound: true
    }
  }

  try {
    const data = await ssrApiClient.organizations.getNotesPublicNotes().request(org, parsedNoteId)

    return {
      props: {
        note: data
      },
      revalidate: 86400
    }
  } catch (e) {
    return {
      notFound: true,
      revalidate: 5
    }
  }
}
