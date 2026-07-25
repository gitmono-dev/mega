import React, { useCallback, useEffect, useMemo, useState } from 'react'

import { Button, UIText } from '@gitmono/ui'
import { Dialog } from '@gitmono/ui/Dialog'

import { usePostRepoClone } from '@/hooks/usePostRepoClone'

import { formatGitHubRepoUrl, isProjectSyncPath, parseGitHubRepoUrl } from './syncUtils'

interface Props {
  currentPath?: string
}

const formatBasePath = (path?: string): string => {
  if (!path || path === '/') return '/'
  return path.endsWith('/') ? path : `${path}/`
}

const isFieldValid = (value: string): boolean => value.trim().length > 0

export default function SyncRepoButton({ currentPath }: Props) {
  const [open, setOpen] = useState(false)
  const [githubUrl, setGithubUrl] = useState('')
  const [repoName, setRepoName] = useState('')

  const { mutateAsync, isPending } = usePostRepoClone()

  const basePath = useMemo(() => formatBasePath(currentPath), [currentPath])
  const isProjectSync = useMemo(() => isProjectSyncPath(currentPath), [currentPath])
  const parsedRepo = useMemo(() => parseGitHubRepoUrl(githubUrl), [githubUrl])

  const fullPath = useMemo(() => (repoName ? `${basePath}${repoName}` : basePath), [basePath, repoName])

  const urlError = useMemo(() => {
    if (!githubUrl.trim() || parsedRepo) return null
    return 'Enter a valid GitHub URL (e.g. https://github.com/owner/repo)'
  }, [githubUrl, parsedRepo])

  const canSubmit = useMemo(
    () => Boolean(parsedRepo) && isFieldValid(repoName) && !isPending,
    [parsedRepo, repoName, isPending]
  )

  const resetForm = useCallback(() => {
    setGithubUrl('')
    setRepoName('')
  }, [])

  const handleOpenChange = useCallback(
    (isOpen: boolean) => {
      setOpen(isOpen)
      if (!isOpen) {
        resetForm()
      }
    },
    [resetForm]
  )

  const handleSync = useCallback(async () => {
    if (!parsedRepo) return

    await mutateAsync({
      owner: parsedRepo.owner,
      repo: parsedRepo.repo,
      path: fullPath.trim()
    })

    handleOpenChange(false)
  }, [parsedRepo, fullPath, mutateAsync, handleOpenChange])

  useEffect(() => {
    if (parsedRepo?.repo) {
      setRepoName(parsedRepo.repo)
    }
  }, [parsedRepo?.repo])

  return (
    <>
      <Button variant='base' onClick={() => setOpen(true)}>
        Sync Repository
      </Button>

      <Dialog.Root open={open} onOpenChange={handleOpenChange}>
        <Dialog.Content>
          <Dialog.CloseButton />
          <Dialog.Header>
            <Dialog.Title>Sync Repository</Dialog.Title>
            <Dialog.Description>{/* Sync a repository from GitHub to your server */}</Dialog.Description>
          </Dialog.Header>

          <div className='flex flex-col gap-4 py-2'>
            {isProjectSync && (
              <UIText quaternary size='text-sm'>
                Syncing under /project creates a Change List and may trigger build checks.
              </UIText>
            )}
            <div className='flex flex-col gap-2'>
              <label className='text-quaternary text-sm'>
                GitHub URL <span className='text-red-500'>*</span>
              </label>
              <input
                className='rounded-md border px-3 py-2 text-sm outline-hidden focus:ring-2'
                value={githubUrl}
                onChange={(e) => setGithubUrl(e.target.value)}
                placeholder='https://github.com/owner/repo'
                disabled={isPending}
              />
              {urlError && (
                <UIText size='text-xs' className='text-red-500'>
                  {urlError}
                </UIText>
              )}
            </div>

            <div className='flex flex-col gap-2'>
              <label className='text-quaternary text-sm'>
                Target Path <span className='text-red-500'>*</span>
              </label>
              <div className='border-primary flex items-center gap-0 overflow-hidden rounded-md border focus-within:ring-2'>
                <span className='border-primary bg-tertiary text-secondary border-r px-3 py-2 text-sm'>{basePath}</span>
                <input
                  className='flex-1 border-none px-3 py-2 text-sm outline-hidden'
                  value={repoName}
                  onChange={(e) => setRepoName(e.target.value)}
                  placeholder='repo-name'
                  disabled={isPending}
                />
              </div>
            </div>

            {parsedRepo && (
              <div className='bg-secondary rounded-md p-3'>
                <UIText quaternary size='text-xs' className='mb-1'>
                  Will sync from:
                </UIText>
                <UIText size='text-sm' className='font-mono'>
                  {formatGitHubRepoUrl(parsedRepo.owner, parsedRepo.repo)}
                </UIText>
                <UIText quaternary size='text-xs' className='mt-2'>
                  To: {fullPath}
                </UIText>
              </div>
            )}
          </div>

          <Dialog.Footer>
            <Dialog.TrailingActions>
              <Button variant='plain' onClick={() => handleOpenChange(false)} disabled={isPending}>
                Cancel
              </Button>

              <Button disabled={!canSubmit} onClick={handleSync} loading={isPending}>
                {isPending ? 'Syncing...' : 'Start Sync'}
              </Button>
            </Dialog.TrailingActions>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Root>
    </>
  )
}
