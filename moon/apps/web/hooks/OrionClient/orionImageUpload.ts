import { createSHA256 } from 'hash-wasm'

/** Stream SHA-256 of a File/Blob without loading it entirely into memory. */
export async function sha256HexOfFile(file: Blob, onProgress?: (ratio: number) => void): Promise<string> {
  const hasher = await createSHA256()

  hasher.init()
  const total = file.size || 1
  let done = 0

  const stream = file.stream()
  const reader = stream.getReader()

  for (;;) {
    const { done: eof, value } = await reader.read()

    if (eof) break
    if (value) {
      hasher.update(value)
      done += value.byteLength
      onProgress?.(Math.min(1, done / total))
    }
  }

  onProgress?.(1)
  return hasher.digest()
}

/** PUT a blob to an upload URL with progress (0–1).
 * Prefer mono proxy URLs (`/api/v1/orion/images/objects/...`) with credentials.
 */
export function putWithProgress(
  url: string,
  blob: Blob,
  onProgress?: (ratio: number) => void,
  options?: { contentType?: string; withCredentials?: boolean }
): Promise<void> {
  const contentType = options?.contentType
  const withCredentials = options?.withCredentials ?? false

  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()

    xhr.open('PUT', url)
    xhr.withCredentials = withCredentials
    if (contentType) {
      xhr.setRequestHeader('Content-Type', contentType)
    }

    xhr.upload.onprogress = (event) => {
      if (!event.lengthComputable || !onProgress) return
      onProgress(event.total > 0 ? event.loaded / event.total : 0)
    }

    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        onProgress?.(1)
        resolve()
        return
      }
      const detail = (xhr.responseText || '').trim().slice(0, 240)

      reject(new Error(`Upload failed (HTTP ${xhr.status})${detail ? `: ${detail}` : ''}`))
    }

    xhr.onerror = () => {
      reject(new Error('Upload network error'))
    }

    // Avoid browser auto Content-Type from Blob.type when none was requested.
    const body = contentType || !blob.type ? blob : blob.slice(0, blob.size, '')

    xhr.send(body)
  })
}

/** Resolve a mono-relative or absolute put URL against MONO_API_URL. */
export function resolveMonoUploadUrl(putUrlOrPath: string, monoApiBase: string): string {
  if (/^https?:\/\//i.test(putUrlOrPath)) {
    return putUrlOrPath
  }
  const base = monoApiBase.replace(/\/$/, '')
  const path = putUrlOrPath.startsWith('/') ? putUrlOrPath : `/${putUrlOrPath}`

  return `${base}${path}`
}
