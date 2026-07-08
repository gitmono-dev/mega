import { NextApiRequestCookies } from 'next/dist/server/api-utils'

const DEFAULT_CAMPSITE_API_SESSION_COOKIE = '_campsite_api_session'

/**
 * Session cookie name for SSR → Campsite API forwarding.
 * Prefer server-only `CAMPSITE_API_SESSION_COOKIE` so dev/k8s can override without rebuilding.
 */
export function getCampsiteApiSessionCookieName(): string {
  return process.env.CAMPSITE_API_SESSION_COOKIE || DEFAULT_CAMPSITE_API_SESSION_COOKIE
}

export const SsrSecretHeader: Record<string, string> = { 'x-campsite-ssr-secret': process.env.SSR_SECRET || '' }

export function apiCookieHeaders(cookies: NextApiRequestCookies) {
  let headers: Record<string, string> = {}
  const cookieName = getCampsiteApiSessionCookieName()

  const sessionCookie = cookies[cookieName]
  if (sessionCookie) {
    const apiCookie = encodeURIComponent(sessionCookie)

    headers['Cookie'] = `${cookieName}=${apiCookie}`
  }

  return { ...headers, ...SsrSecretHeader }
}
