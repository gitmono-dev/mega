import { NextApiRequestCookies } from 'next/dist/server/api-utils'

import { CAMPSITE_API_SESSION_COOKIE } from '@gitmono/config'

export const SsrSecretHeader: Record<string, string> = { 'x-campsite-ssr-secret': process.env.SSR_SECRET || '' }

export function apiCookieHeaders(cookies: NextApiRequestCookies) {
  let headers: Record<string, string> = {}

  if (cookies[CAMPSITE_API_SESSION_COOKIE]) {
    const apiCookie = encodeURIComponent(cookies[CAMPSITE_API_SESSION_COOKIE])

    headers['Cookie'] = `${CAMPSITE_API_SESSION_COOKIE}=${apiCookie}`
  }

  return { ...headers, ...SsrSecretHeader }
}
