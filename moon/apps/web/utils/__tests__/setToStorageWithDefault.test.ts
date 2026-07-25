import { describe, expect, it } from 'vitest'

import { setToStorageWithDefault } from '../setToStorageWithDefault'

/** Node 22+ may leave window.localStorage undefined without --localstorage-file; use an in-memory Storage. */
function createMemoryStorage(): Storage {
  const store = new Map<string, string>()

  return {
    get length() {
      return store.size
    },
    clear() {
      store.clear()
    },
    getItem(key: string) {
      return store.has(key) ? store.get(key)! : null
    },
    key(index: number) {
      return [...store.keys()][index] ?? null
    },
    removeItem(key: string) {
      store.delete(key)
    },
    setItem(key: string, value: string) {
      store.set(key, String(value))
    }
  }
}

describe('setToStorageWithDefault', () => {
  it('it stores JSON', async () => {
    const ls = createMemoryStorage()

    setToStorageWithDefault(ls, 'test', { a: 4 }, { a: 1 })
    expect(ls.getItem('test')).toEqual(JSON.stringify({ a: 4 }))
  })

  it('it removes null', async () => {
    const ls = createMemoryStorage()

    setToStorageWithDefault(ls, 'test', null, { a: 1 })
    expect(ls.getItem('test')).toBeNull()
  })

  it('it removes initial', async () => {
    const ls = createMemoryStorage()

    setToStorageWithDefault(ls, 'test', { a: 1 }, { a: 1 })
    expect(ls.getItem('test')).toBeNull()
  })
})
