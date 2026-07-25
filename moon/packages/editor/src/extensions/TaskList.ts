import { TaskList as TiptapTaskList } from '@tiptap/extension-list'

import { createMarkdownParserSpec } from '../utils/createMarkdownParser'

export const TaskList = TiptapTaskList.extend({
  markdownParseSpec() {
    return createMarkdownParserSpec({ block: TiptapTaskList.name })
  },

  markdownToken: 'task_list'
}).configure({
  HTMLAttributes: {
    class: 'task-list'
  }
})
