import toast from 'react-hot-toast'

import { ApiError, ApiErrorTypes } from '@gitmono/types'

function toastMessage(error: Error): string {
  const raw = error.message?.trim()

  if (raw) return raw

  if (error instanceof ApiError) {
    if (error.name === ApiErrorTypes.ForbiddenError) {
      return 'Access forbidden'
    }
    if (error.name === ApiErrorTypes.AuthenticationError) {
      return 'Please sign in again'
    }
  }

  return 'Something went wrong'
}

export function apiErrorToast(error: Error) {
  // never toast when there are connection stability errors
  if (error instanceof ApiError && error.name === ApiErrorTypes.ConnectionError) {
    return
  }
  toast.error(toastMessage(error))
}
