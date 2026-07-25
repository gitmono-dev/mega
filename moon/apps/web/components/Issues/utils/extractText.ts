import React from 'react'

export function extractTextArray(node: React.ReactNode): string[] {
  if (typeof node === 'string' || typeof node === 'number') {
    return [node.toString()]
  }

  if (Array.isArray(node)) {
    // 递归展开所有节点，返回扁平化的字符串数组
    return node.flatMap(extractTextArray)
  }

  if (React.isValidElement<{ children?: React.ReactNode }>(node)) {
    // 递归提取 React 元素的 children
    return extractTextArray(node.props.children)
  }

  // 其他情况（null, undefined, boolean, function）
  return []
}
