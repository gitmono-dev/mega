'use client'

/** Client `hostname` is the WS URL (e.g. wss://orion.example/ws); scheduler keys VMs by that host. */
export function domainFromClientHostname(hostname: string): string | null {
  const raw = hostname.trim()

  if (!raw) return null

  try {
    const url = new URL(raw.includes('://') ? raw : `ws://${raw}`)

    return url.hostname || null
  } catch {
    const host = raw.split('/')[0]?.split(':')[0]

    return host || null
  }
}

/** Orion host for this mega-ui deploy (matches scheduler VM `domain` / Start Runner server_ws). */
export function localOrionDomainFromUrl(orionApiUrl: string): string | null {
  const raw = orionApiUrl.trim()

  if (!raw) return null

  try {
    const url = new URL(raw.includes('://') ? raw : `https://${raw}`)
    const host = url.hostname?.toLowerCase()

    return host || null
  } catch {
    return null
  }
}

export function isLocalEnvironmentDomain(
  domain: string | null | undefined,
  localOrionDomain: string | null | undefined
): boolean {
  if (!domain || !localOrionDomain) return false
  return domain.trim().toLowerCase() === localOrionDomain.trim().toLowerCase()
}
