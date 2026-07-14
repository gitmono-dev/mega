const SITE_URL_PROD = 'https://www.gitmono.com'
const SITE_URL_DEV = 'http://gitmono.test:3003'

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const IS_PRODUCTION = process.env.APP_ENV === 'production'
export const SCOPE_COOKIE_NAME = 'scope'
export const POLL_OPTION_DESCRIPTION_LENGTH = 32

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const IS_NGROK = !!process.env.NEXT_PUBLIC_IS_NGROK

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const WEB_URL = process.env.NEXT_PUBLIC_WEB_URL || 'https://app.gitmega.com'
export const SITE_URL = IS_PRODUCTION ? SITE_URL_PROD : SITE_URL_DEV
// eslint-disable-next-line turbo/no-undeclared-env-vars
export const SYNC_URL = process.env.NEXT_PUBLIC_SYNC_URL || 'wss://sync.gitmega.com'

export const DESKTOP_APP_PROTOCOL = IS_PRODUCTION ? 'campsite://' : 'campsite-dev://'
export const LAST_CLIENT_JS_BUILD_ID_LS_KEY = 'latest-js-time'

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const RAILS_API_URL = process.env.NEXT_PUBLIC_API_URL || 'https://api.gitmega.com'

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const RAILS_INTERNAL_API_URL = process.env.NEXT_PUBLIC_INTERNAL_API_URL || 'https://api.gitmega.com'
// eslint-disable-next-line turbo/no-undeclared-env-vars
export const MONO_API_URL = process.env.NEXT_PUBLIC_MONO_API_URL || 'https://git.gitmega.com'

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const ORION_API_URL = process.env.NEXT_PUBLIC_ORION_API_URL || 'https://orion.gitmega.com'

// eslint-disable-next-line turbo/no-undeclared-env-vars
export const RAILS_AUTH_URL = process.env.NEXT_PUBLIC_AUTH_URL || 'https://auth.gitmega.com'

// Default Campsite API session cookie name (SSR overrides via CAMPSITE_API_SESSION_COOKIE).
export const CAMPSITE_API_SESSION_COOKIE = '_campsite_api_session'

/*
  Not using an env variable because we use this variable in the browser, which
  requires extra config with Next.js to send env variables to the browser.
*/
export const IMGIX_DOMAIN = IS_PRODUCTION ? 'https://gitmono.imgix.net' : 'https://campsite-dev.imgix.net'

export const FIGMA_PLUGIN_URL = 'https://www.figma.com/community/plugin/1108886817260186751'
export const ZAPIER_APP_URL = 'https://zapier.com/apps/campsite/integrations'
export const CAL_DOT_COM_APP_URL = 'https://app.cal.com/apps/campsite'

export const LINEAR_CALLBACK_URL = `${RAILS_API_URL}/v1/integrations/linear/callback`
export const LINEAR_DEV_CLIENT_ID = 'bc6d5e0c459d67c42ae462f1167736da'
export const LINEAR_PROD_CLIENT_ID = '1dcf5d89abd0be6367a24637e100e6b8'
export const LINEAR_CLIENT_ID = IS_PRODUCTION ? LINEAR_PROD_CLIENT_ID : LINEAR_DEV_CLIENT_ID

export const ONBOARDING_STEP_KEY = 'onboardingStep'
export const ONBOARDING_SHARED_POSTS_KEY = 'onboardingPostIds'

export const PUSHER_KEY = IS_PRODUCTION ? '1301e1180de87095b1c0' : '874a1de2f18896929939'
export const PUSHER_APP_CLUSTER = 'us3'

// Key is generated from the VAPID keys in the Rails app but without padding ("=")
export const WEB_PUSH_PUBLIC_KEY =
  'BF151mIoXtZOsN_515tWb1ykezZZn1HIkDP-fwRjhPOyiKl29G4WwEvFWyxwlLuN0YE_TvyIcx5liEctScKX3nI'

const DEFAULT_SEO_TITLE = 'Campsite — Work communication for distributed teams'
const DEFAULT_SEO_DESCRIPTION =
  'Campsite is designed for distributed teams to cut through the noise of daily work — move faster with more transparent, organized, and thoughtful conversations.'
const DEFAULT_SEO_IMAGE = {
  url: `${SITE_URL}/og/default.png`,
  alt: 'Campsite'
}

export const DEFAULT_SEO = {
  metadataBase: new URL(SITE_URL),
  alternates: {
    canonical: './'
  },
  title: DEFAULT_SEO_TITLE,
  description: DEFAULT_SEO_DESCRIPTION,
  openGraph: {
    type: 'website',
    locale: 'en_US',
    url: SITE_URL,
    site_name: 'Campsite', // used by next-seo
    siteName: 'Campsite', // used by next.js
    images: [DEFAULT_SEO_IMAGE]
  },
  twitter: {
    title: DEFAULT_SEO_TITLE,
    description: DEFAULT_SEO_DESCRIPTION,
    images: [DEFAULT_SEO_IMAGE],
    handle: '@trycampsite',
    site: '@trycampsite',
    cardType: 'summary_large_image'
  }
}

export const MAX_FILE_NUMBER = 10
export const ONE_GB = 1024 * 1024 * 1024

export const COMMUNITY_SLUG = 'design'
export const CAMPSITE_SCOPE = 'campsite'

export const COUNTED_ROLES = ['admin', 'member']

export * from './slack'
