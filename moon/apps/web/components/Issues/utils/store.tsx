// import { CookieValueTypes } from 'cookies-next'
import { atom } from 'jotai'

// import { atomFamily } from 'jotai/utils'

// import { atomWithWebStorage } from '@/utils/atomWithWebStorage'

// export type IssueIndexFilterType = 'open' | 'closed' | 'Merged' | 'draft'

// export const filterAtom = atomFamily(
//   ({ scope, part }: { scope: CookieValueTypes; part: string }) =>
//     atomWithWebStorage<IssueIndexFilterType>(`${scope}:${part}-index-filter`, 'open'),
//   (a, b) => a.scope === b.scope && a.part === b.part
// )

// export const filterAtom = atomFamily(
//   ({ part: _part }: { part: string }) => atom<'open' | 'closed'>('open'),
//   (a, b) => a.part === b.part
// )

export const issueIdAtom = atom('')
export const clIdAtom = atom('')

export const FALSE_EDIT_VAL = ''
export const editIdAtom = atom('')

export const refreshAtom = atom(0)
