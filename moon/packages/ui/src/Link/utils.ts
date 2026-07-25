import { os } from '@todesktop/client-core/platform'
import Router from 'next/router'

import { WEB_URL } from '@gitmono/config'

import { isDesktopApp } from '../hooks'

const isWebUrl = (href?: string) => !!href?.startsWith(WEB_URL)
const hasPublicSharePath = (href?: string) => !!href?.includes('/p/')

export function linkType(href?: string) {
  if (isWebUrl(href)) {
    if (hasPublicSharePath(href)) {
      return 'public_share'
    } else {
      return 'internal'
    }
  } else if (isDesktopApp() && isOAuthLink(href)) {
    return 'desktop_oauth'
  }
}

const OAUTH_DOMAINS = [
  'github.com',
  'linkedin.com',
  'facebook.com',
  'twitter.com',
  'google.com',
  'linear.app',
  'slack.com'
]

// ToDesktop has very naive OAuth detection and will override any navigation to OAuth URLs
// To avoid opening e.g. https://github.com/zquestz/omniauth-google-oauth2 in-app, we manually handle this case
function isOAuthLink(href?: string) {
  return !!href?.includes('oauth') && OAUTH_DOMAINS.some((domain) => href?.includes(domain))
}

export function isAppMention(target: HTMLElement) {
  return target.matches('span[data-type="mention"]') && target.getAttribute('data-role') === 'app'
}

export function closestMentionURL(scope: string, target: HTMLElement) {
  const mention = target?.closest('span[data-type="mention"]')

  if (mention && mention instanceof HTMLElement && !isAppMention(mention)) {
    return `${WEB_URL}/${scope}/people/${mention.dataset.username}`
  }
}

export function openBlank(href: string) {
  if (isDesktopApp()) {
    os.openURL(href)
  } else {
    window.open(href, '_blank')
  }
}

function internalPush(href: string) {
  Router.push(href.replace(WEB_URL, ''))
}

export const isMetaClick = (event: MouseEvent) => event.metaKey || event.ctrlKey

export function handleMentionClick(scope: string, event: MouseEvent, isContainerEditable?: boolean) {
  const mentionURL = closestMentionURL(scope, event.target as HTMLElement)

  if (mentionURL) {
    if (isMetaClick(event)) {
      openBlank(mentionURL)
    } else if (!isContainerEditable) {
      internalPush(mentionURL)
    } else {
      // noop normal clicks on editable mentions
      return false
    }

    return true
  }

  return false
}

export function specialLinkClickHandler(scope: string, event: MouseEvent, isContainerEditable?: boolean) {
  if (event.defaultPrevented) return false

  const handledMention = handleMentionClick(scope, event, isContainerEditable)

  if (!handledMention) {
    const anchor = (event.target as HTMLElement)?.closest('a')

    if (!(anchor instanceof HTMLAnchorElement)) {
      return false
    }

    const href = anchor.getAttribute('href')

    if (!href) {
      return false
    }

    const readonlyOrMeta = !isContainerEditable || isMetaClick(event)
    const isDesktop = isDesktopApp()
    const type = linkType(href)

    if (readonlyOrMeta && isDesktop && (type === 'public_share' || type === 'desktop_oauth')) {
      os.openURL(href)
    } else if (anchor.target === '_blank' && type === 'internal') {
      if (isMetaClick(event)) {
        // open internal links in another window when editing with meta click
        openBlank(href)
      } else if (!isContainerEditable) {
        // if a plain click in a non-editable context, push the internal link
        internalPush(href)
      } else {
        return false
      }
    } else if (isContainerEditable && isMetaClick(event)) {
      // if a meta click in an editable context, open the link in a new window
      openBlank(href)
    } else {
      return false
    }
  }

  event.preventDefault()
  event.stopPropagation()

  return true
}
