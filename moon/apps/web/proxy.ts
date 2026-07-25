import { NextRequest, NextResponse } from 'next/server'

import { apiCookieHeaders } from '@/utils/apiCookieHeaders'
import { ssrApiClient } from '@/utils/queryClient'

export async function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl

  // Check if the request is for /mega or any sub-path under /mega
  if (pathname.startsWith('/mega')) {
    try {
      // Parse cookies from request
      const cookies = Object.fromEntries(request.cookies.getAll().map((cookie) => [cookie.name, cookie.value]))
      const headers = apiCookieHeaders(cookies)

      // Get user's organizations
      const organizations = await ssrApiClient.organizationMemberships
        .getOrganizationMemberships()
        .request({ headers })
        .then((res) => res.map((m) => m.organization).filter((o) => o !== null))

      // Auto-join mega if user has no organizations
      if (organizations.length === 0) {
        await ssrApiClient.organizations.postJoinByToken().request('mega', 's3AX1iyAx3sgGNygiM67', { headers })
      }
    } catch (error) {
      // Ignore errors during auto-join
      console.error('Auto-join mega failed:', error)
    }
  }

  return NextResponse.next()
}

export const config = {
  matcher: '/mega/:path*'
}
