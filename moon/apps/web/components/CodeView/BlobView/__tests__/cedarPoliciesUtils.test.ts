import { describe, expect, it } from 'vitest'

import {
  inferPathPatternFromFilePath,
  parseReviewerRules,
  reviewersForPathPattern,
  rewriteReviewersForPathPattern
} from '../cedarPoliciesUtils'

describe('inferPathPatternFromFilePath', () => {
  it('maps root .cedar policy to empty pattern', () => {
    expect(inferPathPatternFromFilePath('.cedar/policies.cedar')).toBe('')
    expect(inferPathPatternFromFilePath('/.cedar/policies.cedar')).toBe('')
  })

  it('maps nested .cedar policy to parent path with trailing slash', () => {
    expect(inferPathPatternFromFilePath('project/svc/.cedar/policies.cedar')).toBe('project/svc/')
    expect(inferPathPatternFromFilePath('/project/svc/.cedar/policies.cedar')).toBe('project/svc/')
  })
})

describe('parseReviewerRules / rewriteReviewersForPathPattern', () => {
  const sample = `permit(action == "code:review", principal, resource)
    when { resource.path.startsWith("") }
    to ["alice", "bob"];

permit(action == "code:review", principal, resource)
    when { resource.path.startsWith("project/svc/") }
    to ["charlie"];
`

  it('parses multiple rules', () => {
    const rules = parseReviewerRules(sample)

    expect(rules).toHaveLength(2)
    expect(rules[0].pathPattern).toBe('')
    expect(rules[0].reviewers).toEqual(['alice', 'bob'])
    expect(rules[1].pathPattern).toBe('project/svc/')
    expect(rules[1].reviewers).toEqual(['charlie'])
  })

  it('reads reviewers for a path pattern', () => {
    expect(reviewersForPathPattern(sample, '')).toEqual(['alice', 'bob'])
    expect(reviewersForPathPattern(sample, 'project/svc/')).toEqual(['charlie'])
    expect(reviewersForPathPattern(sample, 'missing/')).toEqual([])
  })

  it('rewrites one path pattern and keeps others', () => {
    const next = rewriteReviewersForPathPattern(sample, 'project/svc/', ['dave', 'erin'])

    expect(reviewersForPathPattern(next, '')).toEqual(['alice', 'bob'])
    expect(reviewersForPathPattern(next, 'project/svc/')).toEqual(['dave', 'erin'])
  })

  it('appends a rule when path pattern is missing', () => {
    const next = rewriteReviewersForPathPattern(sample, 'other/', ['zoe'])

    expect(reviewersForPathPattern(next, 'other/')).toEqual(['zoe'])
    expect(reviewersForPathPattern(next, '')).toEqual(['alice', 'bob'])
  })
})
