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

/** True when a stored campsite_user_id matches this user. */
export function megaUserHandlesMatch(
  stored: string | null | undefined,
  user?: { id?: string | null; username?: string | null; github_login?: string | null } | null
): boolean {
  if (!stored || !user) return false
  if (user.id && stored === user.id) return true
  // Transitional: pre-backfill rows may still hold github_login / username strings.
  return stored === megaUserHandle(user) || (!!user.username && stored === user.username)
}
