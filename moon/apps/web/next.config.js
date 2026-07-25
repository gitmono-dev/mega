const path = require('path')

/**
 * @type {import('next').NextConfig}
 */

const withBundleAnalyzer = require('@next/bundle-analyzer')({
  enabled: process.env.ANALYZE === 'true'
})

const cspResourcesByDirective = {
  'script-src': [
    "'self'",
    // unsafe-inline required in all environments for Next.js hydration
    // https://github.com/vercel/next.js/discussions/13418
    "'unsafe-inline'",
    'todesktop-internal:',
    'blob:',
    // unsafe-eval required in development for hot module reloading
    "'unsafe-eval'",
    process.env.NODE_ENV !== 'production' && 'https://cdn.vercel-insights.com'
  ],
  'style-src': [
    "'self'",
    // Inline styles (style-src 'unsafe-inline') are required in all environments for react-hot-toast and tip-tap.
    // https://github.com/timolins/react-hot-toast/issues/138
    // They're also required in development for hot module reloading.
    "'unsafe-inline'"
  ],
  'object-src': ["'none'"],
  'base-uri': ["'self'"],
  'connect-src': [
    "'self'",
    'blob:',
    // Staging and production environments
    'wss://*.gitmega.com',
    'https://*.gitmega.com',
    'http://*.gitmega.com',
    // Crates Pro environments
    'https://*.xuanwu.openatom.cn',
    'wss://*.xuanwu.openatom.cn',
    // gitmono environments
    'https://*.gitmono.com',
    'wss://*.gitmono.com',
    // buck2hub environments
    'https://*.buck2hub.com',
    'wss://*.buck2hub.com',
    // Local demo environments
    'http://*.gitmono.local:8004',
    'http://*.gitmono.local:8000',
    'http://*.gitmono.local:18080',
    // Local development environments
    'http://*.gitmono.test:3001',
    'http://*.gitmono.test:8000',
    'http://*.gitmono.test:8004',
    'http://*.gitmono.local:3001',
    'http://*.gitmega.nju:8080',
    'http://*.gitmega.nju',
    process.env.NODE_ENV !== 'production' && 'localhost:8000',
    process.env.NODE_ENV !== 'production' && 'ws://localhost:9000',
    'https://gitmono.s3.ap-southeast-2.amazonaws.com',
    process.env.NODE_ENV !== 'production' && 'https://campsite-media-dev.s3.amazonaws.com',
    process.env.NODE_ENV !== 'production' && 'd1tk25h31rf8pv.cloudfront.net', // campsite-hls-dev
    'd2m0evjsyl9ile.cloudfront.net', // campsite-hls
    'https://vercel-vitals.axiom.co',
    'https://cdn.vercel-insights.com',
    'https://vitals.vercel-insights.com',
    'https://*.lottiefiles.com',
    'https://*.pusher.com',
    'wss://*.pusher.com',
    'https://gitmono.imgix.net',
    process.env.NODE_ENV !== 'production' && 'https://campsite-dev.imgix.net',
    'https://react-tweet.vercel.app', // for react-tweet embeds
    'https://media.tenor.com' // used for Tenor gifs
  ],
  'font-src': ["'self'"],
  'img-src': [
    "'self'",
    'blob:',
    'data:',
    'https://gitmono.imgix.net',
    'https://gitmono.imgix.video',
    'https://campsite-api.imgix.net',
    'https://lh3.googleusercontent.com',
    'https://public.linear.app', // used for linear issue avatars
    'https://avatars.githubusercontent.com', // used for GitHub issue avatars
    'https://pbs.twimg.com', // used for Twitter avatars
    'https://abs.twimg.com', // used for Tweet previews
    process.env.NODE_ENV !== 'production' && 'https://campsite-dev.imgix.net',
    process.env.NODE_ENV !== 'production' && 'https://campsite-dev.imgix.video',
    process.env.NODE_ENV !== 'production' && 'https://campsite-api-dev.imgix.net',
    'https://*.gitmega.com',
    'https://media.tenor.com' // used for Tenor gifs,
  ],
  'manifest-src': ["'self'"],
  'media-src': [
    "'self'",
    'blob:',
    'data:',
    'd2m0evjsyl9ile.cloudfront.net', // campsite-hls
    process.env.NODE_ENV !== 'production' && 'd1tk25h31rf8pv.cloudfront.net', // campsite-hls-dev
    'https://gitmono.imgix.net',
    'https://campsite-api.imgix.net',
    'https://video.twimg.com', // used for Twitter videos
    process.env.NODE_ENV !== 'production' && 'https://campsite-dev.imgix.net',
    process.env.NODE_ENV !== 'production' && 'https://campsite-api-dev.imgix.net',
    'https://media.tenor.com' // used for Tenor gifs
  ],
  'worker-src': ["'self'", 'blob:']
}

const ContentSecurityPolicy = Object.keys(cspResourcesByDirective).reduce((prevPolicyString, directive) => {
  const resourcesString = cspResourcesByDirective[directive].filter(Boolean).join(' ')

  return prevPolicyString + `${directive} ${resourcesString}; `
}, '')

/** @type {import('next').NextConfig} */
const moduleExports = {
  output: 'standalone',
  experimental: {
    // Keep monorepo package resolution working for local workspace sources.
    externalDir: true
  },
  transpilePackages: [
    '@gitmono/ui',
    '@gitmono/types',
    '@gitmono/config',
    '@gitmono/regex',
    '@gitmono/editor',
    'react-tweet',
    '@primer/react',
    '@pierre/diffs',
    'lru_map'
  ],
  reactStrictMode: true,
  images: {
    unoptimized: true,
    remotePatterns: [
      { protocol: 'https', hostname: 'app.gitmono.com' },
      { protocol: 'https', hostname: 'app.gitmono.test' },
      { protocol: 'https', hostname: 'avatars.slack-edge.com' },
      { protocol: 'https', hostname: 'gitmono.imgix.net' },
      { protocol: 'https', hostname: 'campsite-dev.imgix.net' },
      { protocol: 'https', hostname: 'lh3.googleusercontent.com' },
      { protocol: 'https', hostname: 'uploads.linear.app' }
    ]
  },
  async redirects() {
    return [
      {
        source: '/:org/invitation',
        destination: '/:org',
        permanent: false
      },
      {
        source: '/:org/inbox2',
        destination: `/:org/inbox/updates`,
        permanent: true
      },
      {
        source: '/:org/inbox',
        destination: `/:org/inbox/updates`,
        permanent: true
      },
      {
        source: '/:org/settings/people/:role*',
        destination: `/:org/people`,
        permanent: true
      },
      {
        source: '/:org/settings/tags',
        destination: `/:org/settings`,
        permanent: true
      },
      {
        source: '/:org/settings/projects',
        destination: `/:org/projects`,
        permanent: true
      },
      {
        source: '/:org/onboard/spaces',
        destination: `/:org/onboard/channels`,
        permanent: true
      }
    ]
  },
  async headers() {
    return [
      {
        // Apply these headers to all routes in your application.
        source: '/:path*',
        headers: [
          {
            key: 'Content-Security-Policy',
            value: ContentSecurityPolicy.replace(/\s{2,}/g, ' ').trim()
          },
          {
            key: 'X-Frame-Options',
            value: 'SAMEORIGIN'
          }
        ]
      }
    ]
  },
  async rewrites() {
    return [
      /**
       * @see https://posthog.com/docs/advanced/proxy/nextjs
       */
      {
        source: '/ingest/static/:path*',
        destination: 'https://us-assets.i.posthog.com/static/:path*'
      },
      {
        source: '/ingest/:path*',
        destination: 'https://us.i.posthog.com/:path*'
      },
      {
        source: '/ingest/decide',
        destination: 'https://us.i.posthog.com/decide'
      },
      {
        source: '/sse/:path*',
        destination: `${process.env.NEXT_PUBLIC_ORION_API_URL || 'https://orion.gitmega.com'}/:path*`
      }
    ]
  },
  // This is required to support PostHog trailing slash API requests
  skipTrailingSlashRedirect: true,
  webpack(config, { dev, isServer, webpack }) {
    if (dev && !isServer && process.env.ENABLE_WDYR === '1') {
      const originalEntry = config.entry

      config.entry = async () => {
        const wdrPath = path.resolve(__dirname, './wdyr.ts')
        const entries = await originalEntry()

        if (entries['main.js'] && !entries['main.js'].includes(wdrPath)) {
          entries['main.js'].push(wdrPath)
        }
        return entries
      }
    }

    config.plugins.push(
      new webpack.DefinePlugin({
        'process.env.NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA': JSON.stringify(process.env.NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA)
      })
    )

    return config
  }
}

module.exports = withBundleAnalyzer(moduleExports)
