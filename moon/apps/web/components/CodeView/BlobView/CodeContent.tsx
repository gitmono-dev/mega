'use client'

import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { Avatar } from '@mui/material'
import { motion } from 'framer-motion'
import { useTheme } from 'next-themes'
import { useRouter as useNextRouter } from 'next/dist/client/router'
import toast from 'react-hot-toast'
import { codeToTokens } from 'shiki'

import { UsersIcon } from '@gitmono/ui'

import ThemedMarkdown from '@/components/Theme/ThemedMarkdown/index'
import { useGetBlame } from '@/hooks/useGetBlame'
import { useMemberByActor } from '@/hooks/useMemberByActor'
import { megaUserHandle } from '@/utils/megaUser'
import { getLanguageForFile } from '@/utils/shikiLanguageFallback'

import BlobEditor from './BlobEditor'
import styles from './CodeContent.module.css'

type ViewMode = 'code' | 'blame' | 'preview'

const UserAvatar = React.memo(({ username, zIndex }: { username?: string; zIndex?: number }) => {
  const { data: member } = useMemberByActor(username)
  const displayName = megaUserHandle(member?.user, username || '')
  const avatarUrl = member?.user?.avatar_urls?.sm || member?.user?.avatar_urls?.base || ''

  return (
    <motion.div>
      <Avatar
        alt={displayName || username}
        src={avatarUrl}
        sx={{ width: 20, height: 20, border: '2px solid var(--bg-primary)' }}
        style={{ zIndex }}
      />
    </motion.div>
  )
})

const UserAvatarGroup = React.memo(
  ({
    contributors
  }: {
    contributors: Array<{
      email: string
      username?: string | null
    }>
  }) => {
    return (
      <motion.div
        className='flex items-center justify-center'
        initial='stacked'
        whileHover='spread'
        style={{
          width: 20,
          height: 20,
          position: 'relative'
        }}
      >
        {contributors.map((c, i) => (
          <motion.div
            key={c.username}
            variants={{
              stacked: { x: -i * 12 },
              spread: { x: i * 3 }
            }}
            transition={{ type: 'spring', stiffness: 300, damping: 20 }}
            style={{ position: 'relative' }}
          >
            <UserAvatar username={c.username || undefined} zIndex={i} />
          </motion.div>
        ))}
      </motion.div>
    )
  }
)

UserAvatar.displayName = 'UserAvatar'
UserAvatarGroup.displayName = 'UserAvatarGroup'

const CodeContent = ({
  fileContent,
  path,
  isCodeLoading
}: {
  fileContent: string
  path?: string[]
  isCodeLoading: boolean
}) => {
  const [lfs, setLfs] = useState(false)
  const [selectedLine, setSelectedLine] = useState<number | null>(null)
  const [manualViewMode, setManualViewMode] = useState<ViewMode | null>(null)
  const [isEditMode, setIsEditMode] = useState(false)

  const [shikiTokens, setShikiTokens] = useState<Array<Array<{ content: string; color?: string }>>>([])
  const [isShikiLoading, setIsShikiLoading] = useState(false)

  const nextRouter = useNextRouter()
  const { theme, resolvedTheme } = useTheme()

  const currentTheme = useMemo(() => {
    if (theme === 'system') {
      return resolvedTheme || 'light'
    }
    return theme || 'light'
  }, [theme, resolvedTheme])

  const filePath = useMemo(() => path?.join('/') || '', [path])
  const version = (nextRouter.query.version as string) || 'main'

  const { data: blameData, isLoading: isBlameLoading } = useGetBlame({
    refs: version,
    path: filePath,
    page: 1
  })

  const filename = useMemo(() => {
    if (!path || path.length === 0) {
      return ''
    }
    return path[path.length - 1]
  }, [path])

  const isMarkdownFile = useMemo(() => {
    return filename.toLowerCase().endsWith('.md')
  }, [filename])

  const viewMode = useMemo<ViewMode>(() => {
    if (manualViewMode) return manualViewMode
    return isMarkdownFile ? 'preview' : 'code'
  }, [manualViewMode, isMarkdownFile])

  useEffect(() => {
    setManualViewMode(null)
    setIsEditMode(false)
  }, [path])

  const detectedLanguage = useMemo(() => getLanguageForFile(filename), [filename])

  useEffect(() => {
    if (isCodeLoading) return

    if (!fileContent) {
      setShikiTokens([])
      setIsShikiLoading(false)
      return
    }

    let cancelled = false

    setIsShikiLoading(true)

    const shikiTheme = currentTheme === 'dark' ? 'min-dark' : 'min-light'

    codeToTokens(fileContent, {
      lang: detectedLanguage as any,
      theme: shikiTheme
    })
      .then((result) => {
        if (!cancelled) {
          setShikiTokens(result.tokens)
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsShikiLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [fileContent, detectedLanguage, isCodeLoading, currentTheme])

  useEffect(() => {
    setLfs(isLfsContent(fileContent))
  }, [fileContent])

  const handleLineClick = useCallback(
    (lineNumber: number) => {
      setSelectedLine(lineNumber === selectedLine ? null : lineNumber)
    },
    [selectedLine]
  )

  const handleCopyLine = (line: string) => {
    if (navigator.clipboard) {
      navigator.clipboard
        .writeText(line)
        .then(() => toast.success('Copied to clipboard'))
        .catch(() => toast.error('Copied failed'))
    } else {
      const textarea = document.createElement('textarea')

      textarea.value = line
      document.body.appendChild(textarea)
      textarea.select()
      try {
        document.execCommand('copy')
        toast.success('Copied to clipboard')
      } catch {
        toast.error('Copied failed')
      }
    }
  }

  const handleCopy = () => {
    handleCopyLine(fileContent)
  }

  const handleRawView = () => {
    const newWindow = window.open()

    if (newWindow) {
      newWindow.document.write(`
        <!DOCTYPE html>
        <html lang="en">
          <head>
            <title>Raw Content</title>
            <style>
              body {
                font-family: monospace;
                white-space: pre;
                padding: 20px;
              }
            </style>
          </head>
          <body>${fileContent.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</body>
        </html>
      `)
      newWindow.document.close()
    } else {
      toast.error('Unable to open new window. Please check your browser settings.')
    }
  }

  const handleDownload = () => {
    const blob = new Blob([fileContent], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')

    a.href = url
    a.download = filename

    // Append to the document, click it, and remove it
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)

    // Release the URL object
    URL.revokeObjectURL(url)
  }

  function isLfsContent(content: string): boolean {
    const lines = content.split('\n')
    let foundVersion = false
    let foundOid = false
    let foundSize = false

    for (const line of lines) {
      if (line.startsWith('version ')) {
        foundVersion = true
      } else if (line.startsWith('oid sha256:')) {
        foundOid = true
      } else if (line.startsWith('size ')) {
        foundSize = true
      }
      if (foundVersion && foundOid && foundSize) {
        return true
      }
    }
    return false
  }

  const ContributionRecord = ({
    contributors
  }: {
    contributors: Array<{
      email: string
      username?: string | null
    }>
  }) => {
    return (
      <div className='border-primary bg-secondary flex items-center justify-between border-b px-4 py-2'>
        <div className='flex items-center space-x-2'>
          <span className='text-secondary text-xs'>Older</span>
          <div className='flex items-center space-x-1'>
            <div className={`h-3 w-3 ${styles['bg-blame-10']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-9']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-8']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-7']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-6']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-5']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-4']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-3']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-2']}`}></div>
            <div className={`h-3 w-3 ${styles['bg-blame-1']}`}></div>
          </div>
          <span className='text-secondary text-xs'>Newer</span>
        </div>

        <div className='flex items-center space-x-3'>
          <div className='flex items-center space-x-2'>
            <div className='flex -space-x-1'>
              <div className='flex items-center space-x-1'>
                <UserAvatarGroup contributors={contributors}></UserAvatarGroup>
              </div>

              <div className='flex items-center space-x-1 pr-3 pl-2'>
                <UsersIcon size={16} className='text-primary' />
                <span className='text-primary ml-0 text-xs'>Contributors</span>
              </div>

              <span className='bg-tertiary text-primary rounded-full px-2 py-1 text-xs'>
                {contributors?.length || 0}
              </span>
            </div>
          </div>
        </div>
      </div>
    )
  }

  const getBlameColorClass = useCallback(
    (authorTime: number, earliest_commit_time: number, latest_commit_time: number) => {
      if (!authorTime) return styles['bg-blame-1']

      if (earliest_commit_time === latest_commit_time) {
        return styles['bg-blame-10']
      }
      const relativePosition = (authorTime - earliest_commit_time) / (latest_commit_time - earliest_commit_time)
      const colorLevel = Math.min(Math.floor(relativePosition * 10) + 1, 10)

      return styles[`bg-blame-${colorLevel}`]
    },
    []
  )

  const formatRelativeTime = useCallback((authorTime: number) => {
    if (!authorTime) return 'Unknown'

    const now = Date.now() / 1000
    const daysDiff = Math.floor((now - authorTime) / (24 * 60 * 60))

    if (daysDiff < 1) return 'Today'
    if (daysDiff < 7) return `${daysDiff} days ago`
    if (daysDiff < 30) return `${Math.floor(daysDiff / 7)} weeks ago`
    if (daysDiff < 365) return `${Math.floor(daysDiff / 30)} months ago`
    return `${Math.floor(daysDiff / 365)} years ago`
  }, [])

  const processedBlameBlocks = useMemo(() => {
    if (!blameData?.data?.blocks) return []

    return blameData.data.blocks.map((block) => {
      const colorClass = getBlameColorClass(
        block.blame_info?.commit_time || 0,
        blameData.data?.earliest_commit_time || 0,
        blameData.data?.latest_commit_time || 0
      )
      const blockLines = block.content.split('\n')

      return {
        colorClass,
        blameInfo: block.blame_info,
        startLine: block.start_line,
        lines: blockLines.map((content, index) => ({
          content,
          lineNumber: block.start_line + index
        }))
      }
    })
  }, [blameData, getBlameColorClass])

  // Blame tokens state - map lineNumber to tokens
  const [blameTokensMap, setBlameTokensMap] = useState<Map<number, Array<{ content: string; color?: string }>>>(
    new Map()
  )

  // Load Shiki tokens for blame view - optimized to process per block instead of per line
  useEffect(() => {
    if (!processedBlameBlocks.length) return

    let cancelled = false

    const loadBlameTokens = async () => {
      const shikiTheme = currentTheme === 'dark' ? 'min-dark' : 'min-light'

      const results = await Promise.all(
        processedBlameBlocks.map(async (block) => {
          const blockContent = block.lines.map((l) => l.content).join('\n')

          try {
            const result = await codeToTokens(blockContent, {
              lang: detectedLanguage as any,
              theme: shikiTheme
            })

            return { block, tokens: result.tokens }
          } catch (error) {
            return { block, tokens: null }
          }
        })
      )

      if (cancelled) return

      const newMap = new Map<number, Array<{ content: string; color?: string }>>()

      for (const { block, tokens } of results) {
        block.lines.forEach((line, index) => {
          if (tokens) {
            newMap.set(line.lineNumber, tokens[index] || [])
          }
        })
      }

      setBlameTokensMap(newMap)
    }

    loadBlameTokens()

    return () => {
      cancelled = true
    }
  }, [processedBlameBlocks, detectedLanguage, currentTheme])

  const renderCodeView = useCallback(() => {
    if (isCodeLoading || isShikiLoading) {
      return (
        <div className='bg-primary animate-pulse' style={{ flex: 1, padding: '0 16px', overflow: 'hidden' }}>
          {Array.from({ length: 10 }).map((_, index) => (
            // eslint-disable-next-line react/no-array-index-key
            <div key={index} className='flex items-center py-1'>
              <div className='bg-tertiary mr-4 h-4 w-12 rounded'></div>
              <div className='bg-tertiary h-4 rounded' style={{ width: `${Math.random() * 50 + 30}%` }}></div>
            </div>
          ))}
        </div>
      )
    }
    if (lfs) {
      return (
        <div className='flex items-center justify-center p-8'>
          {/* eslint-disable-next-line react/no-unescaped-entities */}
          <span>(Sorry about that, but we can't show files that are this big right now.)</span>
        </div>
      )
    }

    return (
      <div className='flex flex-1 flex-col overflow-hidden'>
        <div style={{ flex: 1, overflow: 'auto' }}>
          <div
            className={`border-primary bg-primary rounded-b-lg border ${styles.codeContainer}`}
            style={{ minWidth: 'fit-content', userSelect: 'text' }}
          >
            {shikiTokens.map((line, index) => {
              return (
                <div
                  // eslint-disable-next-line react/no-array-index-key
                  key={index}
                  className={selectedLine === index ? 'bg-blue-50 dark:bg-blue-950/30' : ''}
                  style={{
                    padding: '0 16px',
                    fontSize: '14px',
                    fontFamily: 'monospace',
                    whiteSpace: 'pre',
                    display: 'flex'
                  }}
                  onClick={() => handleLineClick(index)}
                >
                  <span className='inline-block w-8'>{selectedLine === index ? <div></div> : null}</span>
                  <span className={styles.codeLineNumber}>{index + 1}</span>
                  <span style={{ display: 'inline' }}>
                    {line.map((token, key) => (
                      // eslint-disable-next-line react/no-array-index-key
                      <span key={key} style={{ color: token.color, display: 'inline' }}>
                        {token.content}
                      </span>
                    ))}
                  </span>
                </div>
              )
            })}
          </div>
        </div>
      </div>
    )
  }, [lfs, selectedLine, handleLineClick, isCodeLoading, isShikiLoading, shikiTokens])

  const renderBlameView = useCallback(() => {
    if (isBlameLoading) {
      return (
        <div className='bg-primary animate-pulse' style={{ flex: 1, overflow: 'hidden' }}>
          {Array.from({ length: 10 }).map((_, index) => (
            // eslint-disable-next-line react/no-array-index-key
            <div key={index} className='border-primary flex border-b py-2'>
              <div className='bg-tertiary mx-2 w-1'></div>

              <div className='flex w-[350px] items-center space-x-2 px-3'>
                <div className='bg-tertiary h-4 w-20 rounded'></div>
                <div className='bg-quaternary h-5 w-5 rounded-full'></div>
                <div className='bg-tertiary h-4 w-32 rounded'></div>
              </div>

              <div className='flex flex-1 items-center px-3'>
                <div className='bg-tertiary mr-4 h-4 w-12 rounded'></div>
                <div className='bg-tertiary h-4 rounded' style={{ width: `${Math.random() * 60 + 20}%` }}></div>
              </div>
            </div>
          ))}
        </div>
      )
    }

    if (!blameData?.data) {
      return (
        <div className='flex items-center justify-center p-8'>
          <div className='text-tertiary'>No blame information available</div>
        </div>
      )
    }

    return (
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        <ContributionRecord contributors={blameData.data?.contributors} />
        <div style={{ flex: 1, overflow: 'auto' }}>
          <div
            className={`border-primary bg-primary rounded-b-lg border ${styles.blameContainer}`}
            style={{ minWidth: 'fit-content', userSelect: 'text' }}
          >
            {processedBlameBlocks.map((block, blockIndex) => {
              const isLastBlock = blockIndex === processedBlameBlocks.length - 1

              return (
                <div
                  // eslint-disable-next-line react/no-array-index-key
                  key={`block-${blockIndex}`}
                  className={`transition-colors duration-150 ${isLastBlock ? '' : 'border-primary border-b'}`}
                >
                  <div className='flex'>
                    <div className='flex w-1 shrink-0 items-center'>
                      <div className={`${block.colorClass} h-[99%] w-[95%] rounded-xs`}></div>
                    </div>

                    <div className='border-primary shrink-0 border-r' style={{ width: '350px' }}>
                      <div className='top-0 z-10 flex items-center px-3 py-2'>
                        <span className='text-secondary w-[100px] truncate text-xs'>
                          {formatRelativeTime(block.blameInfo?.commit_time || 0)}
                        </span>
                        <UserAvatar
                          username={block.blameInfo?.author_username || ''}
                          zIndex={block.blameInfo?.commit_time || 0}
                        />
                        <div className='ml-2 flex w-[200px] items-center'>
                          <span className='text-secondary truncate text-xs' title={block.blameInfo?.commit_summary}>
                            {block.blameInfo?.commit_message || 'No commit message'}
                          </span>
                        </div>
                      </div>
                    </div>

                    <div className={`shrink-0 ${block.lines.length === 1 ? 'flex items-center' : ''}`}>
                      {block.lines.map((line) => {
                        const isSelected = selectedLine === line.lineNumber - 1
                        const lineTokens = blameTokensMap.get(line.lineNumber) || []

                        return (
                          <div
                            key={`line-${line.lineNumber}`}
                            className={`flex items-center ${isSelected ? 'bg-blue-50 dark:bg-blue-950/30' : ''}`}
                            onClick={() => handleLineClick(line.lineNumber - 1)}
                            style={{
                              fontSize: '12px',
                              minHeight: '20px'
                            }}
                          >
                            <div
                              className='bg-primary text-tertiary flex shrink-0 items-center justify-center text-xs select-none'
                              style={{ width: '60px' }}
                            >
                              {line.lineNumber}
                            </div>

                            <div
                              className='flex items-center pr-4 pl-3 font-mono text-sm'
                              style={{ whiteSpace: 'pre' }}
                            >
                              <span style={{ display: 'inline' }}>
                                {lineTokens.map((token, key) => (
                                  // eslint-disable-next-line react/no-array-index-key
                                  <span key={key} style={{ color: token.color, display: 'inline' }}>
                                    {token.content}
                                  </span>
                                ))}
                              </span>
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </div>
    )
  }, [
    isBlameLoading,
    blameData?.data,
    processedBlameBlocks,
    formatRelativeTime,
    selectedLine,
    blameTokensMap,
    handleLineClick
  ])

  const renderPreviewView = useCallback(() => {
    if (isCodeLoading) {
      return (
        <div className='bg-primary animate-pulse p-8' style={{ flex: 1, overflow: 'hidden' }}>
          {Array.from({ length: 3 }).map((_, index) => (
            // eslint-disable-next-line react/no-array-index-key
            <div key={index} className='mb-4'>
              <div className='bg-tertiary mb-2 h-6 rounded' style={{ width: `${Math.random() * 30 + 40}%` }}></div>
              <div className='bg-tertiary mb-1 h-4 rounded' style={{ width: `${Math.random() * 20 + 70}%` }}></div>
              <div className='bg-tertiary mb-1 h-4 rounded' style={{ width: `${Math.random() * 20 + 80}%` }}></div>
              <div className='bg-tertiary h-4 rounded' style={{ width: `${Math.random() * 30 + 50}%` }}></div>
            </div>
          ))}
        </div>
      )
    }

    return (
      <ThemedMarkdown
        className={`border-primary overflow-auto rounded-b-lg border p-8 ${styles.previewContainer}`}
        style={{ userSelect: 'text' }}
      >
        {fileContent}
      </ThemedMarkdown>
    )
  }, [fileContent, isCodeLoading])

  const handleEditClick = useCallback(() => {
    setIsEditMode(true)
  }, [])

  const handleCancelEdit = useCallback(() => {
    setIsEditMode(false)
  }, [])

  if (isEditMode) {
    return <BlobEditor fileContent={fileContent} filePath={filePath} fileName={filename} onCancel={handleCancelEdit} />
  }

  return (
    <div className='flex flex-1 flex-col overflow-hidden'>
      <div className={`${styles.toolbar} border-primary rounded-t-lg border-t`} style={{ flexShrink: 0 }}>
        <div className='bg-tertiary m-2 h-8 rounded-lg'>
          {isMarkdownFile && (
            <button
              className={`${styles.toolbarLeftButton} ${viewMode === 'preview' ? styles.active : ''}`}
              onClick={() => setManualViewMode('preview')}
            >
              Preview
            </button>
          )}
          <button
            className={`${styles.toolbarLeftButton} ${viewMode === 'code' ? styles.active : ''}`}
            onClick={() => setManualViewMode('code')}
          >
            Code
          </button>
          <button
            className={`${styles.toolbarLeftButton} ${viewMode === 'blame' ? styles.active : ''}`}
            onClick={() => setManualViewMode('blame')}
          >
            Blame
          </button>
        </div>
        <div className='text-tertiary hidden whitespace-nowrap 2xl:inline'>
          <span
            className='m-2'
            style={{ minWidth: 0 }}
          >{`${fileContent.split('\n').length}   lines  .  2.79  KB`}</span>
        </div>

        <div className='flex-1' />
        <div className='border-primary m-2 h-8 rounded-lg border p-1'>
          <button className={styles.toolbarRightButton} onClick={handleRawView}>
            Raw
          </button>
          <button className={styles.toolbarRightButton} onClick={handleCopy}>
            Copy
          </button>
          <button className={styles.toolbarRightButton} onClick={handleDownload}>
            Download
          </button>
        </div>
        {version === 'main' && !lfs && (
          <div className='border-primary m-2 h-8 rounded-lg border p-1'>
            <button className={styles.toolbarRightButton} onClick={handleEditClick}>
              Edit
            </button>
          </div>
        )}
      </div>
      {viewMode === 'preview' && renderPreviewView()}
      {viewMode === 'code' && renderCodeView()}
      {viewMode === 'blame' && renderBlameView()}
    </div>
  )
}

export default CodeContent
