import * as React from 'react'
import { useCallback, useEffect } from 'react'
import { Box, Skeleton } from '@mui/material'
import { useTreeViewApiRef } from '@mui/x-tree-view'
import { RichTreeView } from '@mui/x-tree-view/RichTreeView'
import { useAtom } from 'jotai'

import { CommonResultVecMuiTreeNode } from '@gitmono/types'

import { expandedNodesAtom, treeAllDataAtom } from './codeTreeAtom'
import { CustomTreeItem } from './CustomTreeItem'
import { convertToTreeData, getDescendantIds } from './TreeUtils'

interface FileTreeProps {
  treeData: CommonResultVecMuiTreeNode['data']
  treeDataLoading: boolean
  onFileClick?: (filePath: string) => void
}

const FileTree = ({ treeData, treeDataLoading, onFileClick }: FileTreeProps) => {
  const apiRef = useTreeViewApiRef()

  const [treeAllData, setTreeAllData] = useAtom(treeAllDataAtom)
  const [expandedNodes, setExpandedNodes] = useAtom(expandedNodesAtom)

  // Process the tree data when API returns
  useEffect(() => {
    if (treeData) {
      const convertedData = convertToTreeData(treeData)

      setTreeAllData(convertedData)
    }
  }, [treeData, setTreeAllData])

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

      setExpandedNodes(newExpandedIds)
    },
    [expandedNodes, treeAllData, setExpandedNodes]
  )

  const handleItemClick = (_e: React.SyntheticEvent | null, itemId: string) => {
    const item = apiRef.current!.getItem(itemId)

    if (item.content_type) {
      // Call the file click handler if this is a file (not directory)
      if (item.content_type === 'file' && onFileClick) {
        onFileClick(item.path || itemId)
      }
    }
  }

  const handleFocusItem = (_e: React.SyntheticEvent | null, itemId: string) => {
    const item = apiRef.current!.getItem(itemId)

    if (item.content_type) {
      apiRef.current?.setItemSelection({
        itemId,
        keepExistingSelection: false
      })

      // Merge the focused item into the existing expanded set (do not replace).
      setExpandedNodes(expandedNodes.includes(itemId) ? expandedNodes : [...expandedNodes, itemId])

      if (item.content_type === 'file' && onFileClick) {
        onFileClick(item.path || itemId)
      }
    }
  }

  return (
    <>
      {treeDataLoading || treeAllData?.length === 0 ? (
        <Box sx={{ display: 'flex', paddingLeft: '16px' }}>
          <Skeleton width='200px' height='30px' />
        </Box>
      ) : (
        <RichTreeView
          apiRef={apiRef}
          items={treeAllData}
          onItemFocus={handleFocusItem}
          onItemClick={handleItemClick}
          expandedItems={expandedNodes}
          onExpandedItemsChange={handleNodeToggle}
          expansionTrigger='iconContainer'
          sx={{ flexGrow: 1, width: '100%', overflow: 'auto' }}
          slots={{
            item: (itemProps) => <CustomTreeItem {...itemProps} />
          }}
        />
      )}
    </>
  )
}

export default FileTree
