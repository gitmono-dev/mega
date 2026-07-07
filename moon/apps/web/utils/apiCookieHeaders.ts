import { NextApiRequestCookies } from 'next/dist/server/api-utils'

import { CAMPSITE_API_SESSION_COOKIE } from '@gitmono/config'

export const SsrSecretHeader: Record<string, string> = { 'x-campsite-ssr-secret': process.env.SSR_SECRET || '' }

export function apiCookieHeaders(cookies: NextApiRequestCookies) {
  let headers: Record<string, string> = {}

  const sessionCookie = cookies[CAMPSITE_API_SESSION_COOKIE]
  if (sessionCookie) {
    const apiCookie = encodeURIComponent(sessionCookie)

    headers['Cookie'] = `${CAMPSITE_API_SESSION_COOKIE}=${apiCookie}`
  }

  return { ...headers, ...SsrSecretHeader }
}
