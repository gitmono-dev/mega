'use client'

import { useMemo } from 'react'

import { CommonResultVecTreeCommitItem } from '@gitmono/types/generated'

import { useGetBlob } from '@/hooks/useGetBlob'
import { useGetTreeCommitInfo } from '@/hooks/useGetTreeCommitInfo'

import SpinnerTable from './TableWithLoading'

export default function CodeView() {
  const { data: TreeCommitInfo } = useGetTreeCommitInfo('/')

  type DirectoryType = NonNullable<CommonResultVecTreeCommitItem['data']>
  const directory: DirectoryType = useMemo(() => TreeCommitInfo?.data ?? [], [TreeCommitInfo])

  const reqPath = `/README.md`
  const { data: readmeContent } = useGetBlob({ path: reqPath })

  return <SpinnerTable isLoading={!TreeCommitInfo} datasource={directory} content={readmeContent?.data} />
}
