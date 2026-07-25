import deepEqual from 'fast-deep-equal'

export function setToStorageWithDefault<T>(storage: Storage | undefined, key: string, value: T, initialValue: T) {
  if (value == null || deepEqual(value, initialValue)) {
    storage?.removeItem(key)
  } else {
    try {
      const stringify = JSON.stringify(value)

      storage?.setItem(key, stringify)
    } catch (error) {
      console.error('Failed to write to storage', { key, error })
      storage?.removeItem(key)
    }
  }
}
