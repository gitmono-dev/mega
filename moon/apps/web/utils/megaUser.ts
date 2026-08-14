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

/**
 * System timeline comments historically prefix the campsite public id, and
 * newer writes may prefix a display label. Strip that leading actor token so
 * the UI can render displayName + phrase separately.
 */
export function systemEventPhrase(comment: string | null | undefined, actor: string, fallback: string): string {
  const text = comment?.trim()

  if (!text) return fallback

  let rest = text
  const actorPrefix = `${actor} `

  if (rest.startsWith(actorPrefix)) {
    rest = rest.slice(actorPrefix.length)
  }

  if (rest === fallback) return fallback
  if (rest.endsWith(` ${fallback}`)) return fallback

  // "{displayLabel} marked this as draft|ready for review|..."
  const space = rest.indexOf(' ')

  if (space > 0) {
    const after = rest.slice(space + 1)

    if (
      after === fallback ||
      after.startsWith('marked this as ') ||
      after === 'closed this' ||
      after === 'reopen this' ||
      after === 'mentioned this on'
    ) {
      return after
    }
  }

  return rest
}
