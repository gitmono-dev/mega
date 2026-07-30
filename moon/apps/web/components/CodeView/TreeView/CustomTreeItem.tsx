import * as React from 'react'
import { CircularProgress } from '@mui/material'
import { alpha, styled } from '@mui/material/styles'
import {
  TreeItemDragAndDropOverlay,
  TreeItemIcon,
  TreeItemProvider,
  useTreeItem,
  useTreeItemModel,
  UseTreeItemParameters
} from '@mui/x-tree-view'
import {
  TreeItemContent,
  TreeItemGroupTransition,
  TreeItemIconContainer,
  TreeItemRoot
} from '@mui/x-tree-view/TreeItem'

import { CustomLabel } from './CustomLabel'
import { getIconFromFileType, MuiTreeNode } from './TreeUtils'

interface CustomTreeItemProps
  extends Omit<UseTreeItemParameters, 'rootRef'>, Omit<React.HTMLAttributes<HTMLLIElement>, 'onFocus'> {}

export const LoadingDirectoriesContext = React.createContext<Set<string>>(new Set())

// Must live outside the component — defining styled() during render creates a new
// component type every time and remounts the subtree (visible tree "jump").
const StyledGroupTransition = styled(TreeItemGroupTransition)(({ theme }) => ({
  marginLeft: 15,
  borderLeft: `1px dashed ${alpha(theme.palette.text.primary, 0.4)}`
}))

// Custom tree structure node component, used to render elements such as icons and labels for each node
export const CustomTreeItem = React.forwardRef(function CustomTreeItem(
  props: CustomTreeItemProps,
  ref: React.Ref<HTMLLIElement>
) {
  const { id, itemId, label, disabled, children, ...other } = props
  const {
    getContextProviderProps,
    getRootProps,
    getContentProps,
    getIconContainerProps,
    getLabelProps,
    getGroupTransitionProps,
    getDragAndDropOverlayProps,
    status
  } = useTreeItem({ id, itemId, children, label, disabled, rootRef: ref })

  const item = useTreeItemModel<MuiTreeNode>(itemId)!
  const loadingDirectories = React.useContext(LoadingDirectoriesContext)

  // If it is a placeholder node, no content is rendered
  if (item.isPlaceholder) {
    return null
  }

  let icon

  if (item.content_type === 'directory') {
    icon = getIconFromFileType(item.content_type, status.expanded)
  } else {
    icon = getIconFromFileType(item.content_type, false)
  }

  // Check if the current node is loading
  const isNodeLoading = loadingDirectories.has(item.path)

  return (
    <TreeItemProvider {...getContextProviderProps()}>
      <TreeItemRoot {...getRootProps(other)}>
        <TreeItemContent {...getContentProps()} sx={{ paddingLeft: 1 }}>
          <TreeItemIconContainer {...getIconContainerProps()}>
            {isNodeLoading ? <CircularProgress size={12} sx={{ color: 'black' }} /> : <TreeItemIcon status={status} />}
          </TreeItemIconContainer>

          <CustomLabel
            {...getLabelProps({
              icon
            })}
          />

          <TreeItemDragAndDropOverlay {...getDragAndDropOverlayProps()} />
        </TreeItemContent>
        {children && <StyledGroupTransition {...getGroupTransitionProps()} />}
      </TreeItemRoot>
    </TreeItemProvider>
  )
})
