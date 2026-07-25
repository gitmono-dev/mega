import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useAtom } from 'jotai'
import { atomFamily } from 'jotai/utils'

import { CommonPageDiffItemSchema, FilesChangedPage } from '@gitmono/types'

import { atomWithWebStorage } from '@/utils/atomWithWebStorage'
import { legacyApiClient } from '@/utils/queryClient'

const fetchClFileChanged = legacyApiClient.v1.postApiClFilesChanged()
const getFileChangedAtom = atomFamily((link: string) =>
  atomWithWebStorage<FilesChangedPage['page']>(`${link}:file-changed`, {
    total: 0,
    items: []
  })
)

export const useGetClFileChanged = (
  link: string,
  pagination: {
    page: number
    per_page: number
  }
) => {
  const [, setFileChanged] = useAtom(getFileChangedAtom(link))

  const { data, isLoading } = useQuery({
    queryKey: [...fetchClFileChanged.requestKey(link), pagination.page, pagination.per_page],
    queryFn: async () => {
      const result = await fetchClFileChanged.request(link, {
        additional: '',
        pagination
      })

      return (
        result.data?.page ?? {
          total: 0,
          items: []
        }
      )
    },
    enabled: !!link
  })

  const fileChanged: CommonPageDiffItemSchema = useMemo(() => {
    if (data) {
      setFileChanged(data)
    }
    return (
      data ?? {
        total: 0,
        items: []
      }
    )
  }, [data, setFileChanged])

  return {
    fileChanged,
    isLoading
  }
}
