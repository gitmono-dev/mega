import React, { useMemo } from 'react'
import { CopyIcon, FileCodeIcon } from '@primer/octicons-react'
import copy from 'copy-to-clipboard'
import { format, formatDistance, fromUnixTime } from 'date-fns'
import { useAtom } from 'jotai'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { CommitSummary, CommonPageDiffItemSchema, CommonResultVecMuiTreeNode, DiffItemSchema } from '@gitmono/types'
import { LoadingSpinner } from '@gitmono/ui'

import { formatAssignees } from '@/components/CodeView/CommitsView'
import { commitPath } from '@/components/CodeView/CommitsView/items'
import FileDiff from '@/components/DiffView/FileDiff'
import { MemberHoverAvatarList } from '@/components/Issues/MemberHoverAvatarList'
import { useScope } from '@/contexts/scope'
import { useGetCommitsMuiTree } from '@/hooks/commits/useGetCommitsMuiTree'
import { usePostCommitsFilesChanged } from '@/hooks/commits/usePostCommitsFilesChanged'

interface CommitDetailData {
  commit: CommitSummary
  diffs: DiffItemSchema[]
}

export const CommitsDetailView: React.FC = () => {
  const router = useRouter()
  const { sha } = router.query
  const { scope } = useScope()

  const [commitPathValue, _setCommitPath] = useAtom(commitPath)

  const commitSha = Array.isArray(sha) ? sha[0] : (sha ?? '')

  const commitParams = useMemo(() => {
    if (!commitSha || !commitPathValue) return null
    return {
      path: commitPathValue,
      sha: commitSha
    }
  }, [commitPathValue, commitSha])

  const { data: filesChangedRes, isLoading: isLoadingFilesChanged } = usePostCommitsFilesChanged(commitParams, {
    page: 1,
    per_page: 100
  })

  const { data: treeResponse, isLoading: treeIsLoading } = useGetCommitsMuiTree(commitParams)

  const commitsDetail: CommitDetailData | undefined = useMemo(() => {
    if (!filesChangedRes?.data) return undefined
    return {
      commit: filesChangedRes.data.commit,
      diffs: filesChangedRes.data.page.items as DiffItemSchema[]
    }
  }, [filesChangedRes])

  const fileChangeData: CommonPageDiffItemSchema | undefined = useMemo(() => {
    if (!commitsDetail?.diffs) return undefined

    return {
      items: commitsDetail.diffs,
      total: filesChangedRes?.data?.page.total ?? commitsDetail.diffs.length
    }
  }, [commitsDetail, filesChangedRes])

  const stats = useMemo(() => {
    if (!commitsDetail?.diffs) return { additions: 0, deletions: 0, files: 0 }

    let additions = 0
    let deletions = 0

    commitsDetail.diffs.forEach((diff) => {
      const lines = diff.data.split('\n')

      lines.forEach((line) => {
        if (line.startsWith('+') && !line.startsWith('+++')) {
          additions++
        } else if (line.startsWith('-') && !line.startsWith('---')) {
          deletions++
        }
      })
    })

    return {
      additions,
      deletions,
      files: commitsDetail.diffs.length
    }
  }, [commitsDetail])

  const commitDate = commitsDetail?.commit.date ? fromUnixTime(parseInt(commitsDetail.commit.date, 10)) : null

  if (isLoadingFilesChanged && !commitsDetail) {
    return (
      <div className='flex h-[400px] items-center justify-center'>
        <LoadingSpinner />
      </div>
    )
  }

  if (!commitsDetail) {
    return (
      <div className='flex h-[400px] items-center justify-center'>
        <div className='text-center'>
          <p className='text-secondary text-lg'>No commit data found</p>
        </div>
      </div>
    )
  }

  return (
    <div className='flex flex-col'>
      {/* Commit Header */}
      <div className='border-primary bg-primary px-5 py-4 text-2xl'>
        <div className='flex items-center justify-between'>
          <h1 className='text-primary mb-3 pt-5 font-semibold'>
            <span className='pr-2'>Commit</span>
            <code className='bg-secondary text-primary rounded px-2 py-1'>
              {commitsDetail.commit.sha.substring(0, 7)}
            </code>
          </h1>

          <div
            className='border-primary bg-secondary hover:bg-tertiary flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 transition-colors'
            onClick={() => {
              router.push(`/${scope}/code`)
            }}
          >
            <FileCodeIcon size={16} />
            <span className='text-sm'>Browse files</span>
          </div>
        </div>

        <div className='text-secondary flex flex-wrap items-center gap-2 text-sm'>
          <div className='flex items-center gap-2'>
            <MemberHoverAvatarList authors={[commitsDetail.commit.author]} isLeft={true} />
            {formatAssignees([commitsDetail.commit.author])}
            <span>authored</span>
            {commitDate && (
              <time dateTime={commitDate.toISOString()} title={format(commitDate, 'PPpp')}>
                {formatDistance(commitDate, new Date(), { addSuffix: true })}
              </time>
            )}
            {'  ·'}
          </div>

          {commitDate && (
            <span
              className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium ${
                commitsDetail.commit.gpg_status === 'Verified'
                  ? 'border-green-600 text-green-600 dark:border-green-500 dark:text-green-500'
                  : commitsDetail.commit.gpg_status === 'Unverified'
                    ? 'border-yellow-600 text-yellow-600 dark:border-yellow-500 dark:text-yellow-500'
                    : 'border-border-default text-tertiary'
              }`}
            >
              {commitsDetail.commit.gpg_status}
            </span>
          )}
        </div>
      </div>

      {fileChangeData && (
        <div className='flex flex-col px-5'>
          <div className='border-primary justify-center rounded-lg border'>
            <div className='border-primary text-primary border-b px-4 py-2'> {commitsDetail.commit.short_message}</div>

            <div className='flex items-center justify-between px-4 py-2 text-sm'>
              <div className='flex items-center gap-4'>
                <div className='flex items-center gap-2'>
                  <span className='text-primary font-medium'>{stats.files}</span>
                  <span className='text-secondary'>file{stats.files !== 1 ? 's' : ''} changed</span>
                </div>
                <div className='flex items-center gap-2'>
                  <span className='font-medium text-green-600 dark:text-green-500'>+{stats.additions}</span>
                  <span className='text-secondary'>additions</span>
                </div>
                <div className='flex items-center gap-2'>
                  <span className='font-medium text-red-600 dark:text-red-500'>-{stats.deletions}</span>
                  <span className='text-secondary'>deletions</span>
                </div>
              </div>

              <div className='text-secondary flex items-center gap-2 text-sm'>
                {commitsDetail.commit.parents && commitsDetail.commit.parents.length > 0 ? (
                  <>
                    <span>
                      {commitsDetail.commit.parents.length} parent{commitsDetail.commit.parents.length > 1 ? 's' : ''}
                    </span>
                    {commitsDetail.commit.parents.map((parentSha, index) => (
                      <React.Fragment key={parentSha}>
                        <button
                          onClick={() => router.push(`/${scope}/code/commit/${parentSha}`)}
                          className='text-accent font-mono transition-colors hover:underline'
                        >
                          {parentSha.substring(0, 7)}
                        </button>
                        {index < commitsDetail.commit.parents.length - 1 && <span>,</span>}
                      </React.Fragment>
                    ))}
                    <span>commit</span>
                    <code className='text-primary font-mono'>{commitSha.substring(0, 7)}</code>
                    <button
                      onClick={() => {
                        void copy(commitSha).then((ok) => {
                          if (ok) {
                            toast.success('Copied to clipboard')
                          } else {
                            toast.error('Copy failed')
                          }
                        })
                      }}
                      className='text-tertiary hover:text-secondary transition-colors'
                      title='Copy full SHA'
                    >
                      <CopyIcon className='h-4 w-4' />
                    </button>
                  </>
                ) : (
                  <>
                    <span>commit</span>
                    <code className='text-primary font-mono'>{commitSha.substring(0, 7)}</code>
                    <button
                      onClick={() => {
                        void copy(commitSha).then((ok) => {
                          if (ok) {
                            toast.success('Copied to clipboard')
                          } else {
                            toast.error('Copy failed')
                          }
                        })
                      }}
                      className='text-tertiary hover:text-secondary transition-colors'
                      title='Copy full SHA'
                    >
                      <CopyIcon className='h-4 w-4' />
                    </button>
                  </>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* File Diffs */}
      {fileChangeData ? (
        <FileDiff
          fileChangeData={fileChangeData}
          fileChangeIsLoading={isLoadingFilesChanged}
          treeData={treeResponse?.data as CommonResultVecMuiTreeNode['data']}
          treeIsLoading={treeIsLoading}
          page={1}
          hasMoreData={false}
          isBlockingLoading={isLoadingFilesChanged || treeIsLoading}
          onLoadMore={() => undefined}
        />
      ) : (
        <div className='text-tertiary flex h-[200px] items-center justify-center'>No file changes</div>
      )}
    </div>
  )
}

export default CommitsDetailView
