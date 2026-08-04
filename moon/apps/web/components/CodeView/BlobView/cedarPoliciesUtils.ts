/**
 * Parse / rewrite helpers for `.cedar/policies.cedar` reviewer rules.
 * Matches the custom syntax understood by saturn/src/reviewer_parser.rs:
 *
 * permit(action == "code:review", principal, resource)
 *     when { resource.path.startsWith("path/") }
 *     to ["alice", "bob"];
 */

export type CedarReviewerRule = {
  pathPattern: string
  reviewers: string[]
  /** Full matched rule text including trailing semicolon when present */
  raw: string
}

const RULE_PATTERN =
  /permit\s*\([^)]*\)\s*when\s*\{\s*resource\.path\.startsWith\s*\(\s*"([^"]*)"\s*\)\s*\}\s*to\s*\[([^\]]+)\]\s*;?/gs

const REVIEWER_PATTERN = /"([^"]+)"/g

/** Infer startsWith path pattern from a policies.cedar file path. */
export function inferPathPatternFromFilePath(filePath: string): string {
  const normalized = filePath.replace(/\\/g, '/').replace(/^\/+/, '')

  // Expect .../.cedar/policies.cedar
  const cedarSuffix = '/.cedar/policies.cedar'

  if (normalized === '.cedar/policies.cedar' || normalized.endsWith(cedarSuffix)) {
    const parent = normalized.endsWith(cedarSuffix) ? normalized.slice(0, -cedarSuffix.length) : ''

    if (!parent) return ''
    return parent.endsWith('/') ? parent : `${parent}/`
  }

  if (normalized === 'policies.cedar') return ''

  return ''
}

export function parseReviewerRules(content: string): CedarReviewerRule[] {
  const rules: CedarReviewerRule[] = []
  const re = new RegExp(RULE_PATTERN.source, RULE_PATTERN.flags)

  let match: RegExpExecArray | null

  while ((match = re.exec(content)) !== null) {
    const pathPattern = match[1] ?? ''
    const reviewersStr = match[2] ?? ''
    const reviewers: string[] = []
    const reviewerRe = new RegExp(REVIEWER_PATTERN.source, 'g')
    let rMatch: RegExpExecArray | null

    while ((rMatch = reviewerRe.exec(reviewersStr)) !== null) {
      if (rMatch[1]) reviewers.push(rMatch[1])
    }

    if (reviewers.length > 0) {
      rules.push({
        pathPattern,
        reviewers,
        raw: match[0]
      })
    }
  }

  return rules
}

export function reviewersForPathPattern(content: string, pathPattern: string): string[] {
  const normalized = pathPattern.trim()
  const rules = parseReviewerRules(content)
  const matched = rules.filter((r) => r.pathPattern === normalized)

  if (matched.length === 0) return []

  const seen = new Set<string>()
  const out: string[] = []

  for (const rule of matched) {
    for (const name of rule.reviewers) {
      if (!seen.has(name)) {
        seen.add(name)
        out.push(name)
      }
    }
  }

  return out.sort()
}

function formatRule(pathPattern: string, reviewers: string[]): string {
  const list = reviewers.map((r) => `"${r}"`).join(', ')

  return `permit(action == "code:review", principal, resource)
    when { resource.path.startsWith("${pathPattern}") }
    to [${list}];`
}

/**
 * Update or insert the rule for `pathPattern` with the given reviewers.
 * Other path-pattern rules are preserved in order.
 */
export function rewriteReviewersForPathPattern(content: string, pathPattern: string, reviewers: string[]): string {
  if (reviewers.length === 0) {
    return content
  }

  const sortedReviewers = [...reviewers].sort()
  const newRule = formatRule(pathPattern, sortedReviewers)
  const re = new RegExp(RULE_PATTERN.source, RULE_PATTERN.flags)

  const parts: string[] = []
  let lastIndex = 0
  let replaced = false
  let match: RegExpExecArray | null

  while ((match = re.exec(content)) !== null) {
    const rulePath = match[1] ?? ''

    parts.push(content.slice(lastIndex, match.index))

    if (!replaced && rulePath === pathPattern) {
      parts.push(newRule)
      replaced = true
    } else if (rulePath === pathPattern) {
      // Drop duplicate rules for the same path once we've replaced the first.
    } else {
      // Preserve original formatting for other rules.
      parts.push(match[0].endsWith(';') ? match[0] : `${match[0]};`)
    }

    lastIndex = match.index + match[0].length
  }

  parts.push(content.slice(lastIndex))

  if (replaced) {
    return (
      parts
        .join('')
        .replace(/\n{3,}/g, '\n\n')
        .trimEnd() + '\n'
    )
  }

  const trimmed = content.trimEnd()

  if (trimmed.length === 0) {
    return `${newRule}\n`
  }

  return `${trimmed}\n\n${newRule}\n`
}
