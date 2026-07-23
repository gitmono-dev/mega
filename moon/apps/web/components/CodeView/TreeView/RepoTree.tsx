import * as React from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Box, Skeleton } from '@mui/material'
import { useTreeViewApiRef } from '@mui/x-tree-view'
import { RichTreeView } from '@mui/x-tree-view/RichTreeView'
import { useAtom } from 'jotai'
import { usePathname } from 'next/navigation'
import { useRouter } from 'next/router'
import toast from 'react-hot-toast'

import { useGetTree } from '@/hooks/useGetTree'
import { legacyApiClient } from '@/utils/queryClient'

import { expandedNodesAtom, treeAllDataAtom } from './codeTreeAtom'
import { CustomTreeItem } from './CustomTreeItem'
import {
  convertToTreeData,
  findNode,
  generateExpandedPaths,
  getDescendantIds,
  mergeTreeNodes,
  MuiTreeNode
} from './TreeUtils'

const RepoTree = ({ onCommitInfoChange }: { onCommitInfoChange?: Function }) => {
  const router = useRouter()
  const scope = router.query.org as string
  const pathname = usePathname()

  const version = router.query.version as string

  let basePath = pathname?.replace(new RegExp(`\\/${scope}\\/code\\/(tree|blob)`), '') || '/'

  if (version && basePath.startsWith(`/${version}`)) {
    basePath = basePath.substring(`/${version}`.length) || '/'
  }

  const apiRef = useTreeViewApiRef()

  const [treeAllData, setTreeAllData] = useAtom(treeAllDataAtom)
  const [expandedNodes, setExpandedNodes] = useAtom(expandedNodesAtom)
  const [loadingDirectories, setLoadingDirectories] = useState<Set<string>>(new Set())

  const refs = version === 'main' ? undefined : version
  const { data: treeItems, isLoading: isTreeLoading } = useGetTree({ path: basePath, ...(refs ? { refs } : {}) })

  const prevRefsRef = useRef<string | undefined>(refs)
  const loadingRequestsRef = useRef<Set<string>>(new Set())
  const treeAllDataRef = useRef<MuiTreeNode[]>(treeAllData)

  useEffect(() => {
    treeAllDataRef.current = treeAllData
  }, [treeAllData])

  useEffect(() => {
    const refsChanged = prevRefsRef.current !== refs

    if (refsChanged) {
      setTreeAllData([])
      setExpandedNodes(generateExpandedPaths(basePath))
      setLoadingDirectories(new Set([basePath]))

      prevRefsRef.current = refs
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refs])

  useEffect(() => {
    const pathsToExpand = generateExpandedPaths(basePath)
    const existingNode = findNode(treeAllData, basePath)
    const hasRealData = existingNode?.children && !existingNode?.children[0]?.isPlaceholder

    if (
      !loadingDirectories.has(basePath) &&
      (!existingNode || existingNode?.content_type === 'directory') &&
      !hasRealData
    ) {
      setLoadingDirectories((prev) => new Set(prev).add(basePath))
    }

    // Merge ancestors of the current path into the existing expanded set so that
    // navigating into a nested folder does not collapse previously opened parents
    // or sibling branches.
    setExpandedNodes(Array.from(new Set([...expandedNodes, ...pathsToExpand])))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [basePath])

  useEffect(() => {
    if (treeItems) {
      const newPathTreeData = convertToTreeData(treeItems)
      const merged = mergeTreeNodes(treeAllDataRef.current, newPathTreeData)

      setTreeAllData(merged)
    }
  }, [setTreeAllData, treeItems])

  const handleNodeToggle = useCallback(
    (_event: React.SyntheticEvent | null, nodeIds: string[]) => {
      const collapsedNodes = expandedNodes.filter((id) => !nodeIds.includes(id))

      let newExpandedIds = [...nodeIds]

      if (collapsedNodes.length > 0) {
        collapsedNodes.forEach((collapsedId) => {
          const descendantIds = getDescendantIds(treeAllData, collapsedId)

          newExpandedIds = newExpandedIds.filter((id) => !descendantIds.includes(id))
        })
      }

      const newlyExpandedIds = newExpandedIds.filter((id) => !expandedNodes.includes(id))

      newlyExpandedIds.forEach((nodeId) => {
        const existingNode = findNode(treeAllData, nodeId)
        const hasRealData = existingNode?.children && !existingNode?.children[0].isPlaceholder

        if (!loadingDirectories.has(nodeId) && !hasRealData) {
          setLoadingDirectories((prev) => new Set(prev).add(nodeId))
        }
      })

      setExpandedNodes(newExpandedIds)
    },
    [expandedNodes, loadingDirectories, treeAllData, setLoadingDirectories, setExpandedNodes]
  )

  useEffect(() => {
    loadingDirectories.forEach((path) => {
      const requestKey = `${path}:${refs || 'default'}`

      if (loadingRequestsRef.current.has(requestKey)) {
        return
      }

      loadingRequestsRef.current.add(requestKey)

      const params: any = { path }

      if (refs) params.refs = refs

      legacyApiClient.v1
        .getApiTree()
        .request(params)
        .then((response: any) => {
          if (response) {
            const newDirectoryData = convertToTreeData(response)

            const merged = mergeTreeNodes(treeAllDataRef.current, newDirectoryData)

            setTreeAllData(merged)
          }
        })
        .catch((_error: any) => {
          toast.error('Loading failed.')
        })
        .finally(() => {
          loadingRequestsRef.current.delete(requestKey)
          setLoadingDirectories((prev) => {
            const newSet = new Set(prev)

            newSet.delete(path)
            return newSet
          })
        })
    })
  }, [loadingDirectories, refs, setTreeAllData])

  const handleLabelClick = useCallback(
    (path: string, isDirectory: boolean) => {
      if (isDirectory) {
        // Keep ancestors (and this folder) expanded across navigation.
        setExpandedNodes(Array.from(new Set([...expandedNodes, ...generateExpandedPaths(path)])))

        const fullPath = `/${scope}/code/tree/${version}${path}`
        const cleanPath = fullPath.replace(/\/+/g, '/')

        router.push(cleanPath)
      } else {
        const filePath = path.startsWith('/') ? path : `/${path}`

        const blobPath = `/${scope}/code/blob/${version}${filePath}`

        router.push(blobPath)
      }
    },
    [router, scope, version, expandedNodes, setExpandedNodes]
  )

  const handleFocusItem = (_e: React.SyntheticEvent | null, itemId: string) => {
    const item = apiRef.current!.getItem(itemId)

    if (item.content_type) {
      handleLabelClick(item.path, item.content_type === 'directory')
      apiRef.current?.setItemSelection({
        itemId,
        keepExistingSelection: false
      })
    }
  }

  useEffect(() => {
    if (basePath) {
      onCommitInfoChange?.(basePath)
    }
  }, [basePath, onCommitInfoChange])

  useEffect(() => {
    if (apiRef.current && basePath && treeAllData.length > 0) {
      const selectNode = () => {
        try {
          const item = apiRef.current!.getItem(basePath)

          if (item) {
            apiRef.current?.setItemSelection({
              itemId: basePath,
              keepExistingSelection: false
            })
            return true
          }
        } catch (e) {
          return false
        }
        return false
      }

      if (!selectNode()) {
        requestAnimationFrame(() => selectNode())
      }
    }
  }, [basePath, treeAllData, apiRef])

  const showInitialSkeleton = (isTreeLoading || loadingDirectories.has(basePath)) && treeAllData?.length === 0

  return (
    <>
      {showInitialSkeleton ? (
        <Box sx={{ display: 'flex', paddingLeft: '16px' }}>
          <Skeleton width='200px' height='30px' />
        </Box>
      ) : (
        <RichTreeView
          apiRef={apiRef}
          items={treeAllData}
          onItemFocus={handleFocusItem}
          expandedItems={expandedNodes}
          onExpandedItemsChange={handleNodeToggle}
          expansionTrigger='iconContainer'
          sx={{ height: 'fit-content', flexGrow: 1, width: '100%', overflow: 'auto' }}
          slots={{
            item: (itemProps) => <CustomTreeItem {...itemProps} loadingDirectories={loadingDirectories} />
          }}
        />
      )}
    </>
  )
}

export default RepoTree
