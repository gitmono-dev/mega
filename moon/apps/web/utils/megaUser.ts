/** Prefer GitHub login for Mega/CL identity display (falls back to Campsite username). */
export function megaUserHandle(
  user?: { username?: string | null; github_login?: string | null } | null,
  fallback = ''
): string {
  const github = user?.github_login?.trim()

  if (github) return github
  const username = user?.username?.trim()

  if (username) return username
  return fallback
}

/** True when a stored CL/reviewer handle matches this user (username or github_login). */
export function megaUserHandlesMatch(
  stored: string | null | undefined,
  user?: { username?: string | null; github_login?: string | null } | null
): boolean {
  if (!stored || !user) return false
  if (stored === user.username) return true
  const github = user.github_login?.trim()

  return !!github && stored === github
}
