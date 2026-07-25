import { UndoRedo } from '@tiptap/extensions'

/** TipTap 3 renamed History → UndoRedo; keep History export for call sites. */
export const History = UndoRedo
