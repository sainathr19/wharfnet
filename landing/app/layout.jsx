import { Footer, Layout, Navbar } from 'nextra-theme-docs'
import { Head } from 'nextra/components'
import { getPageMap } from 'nextra/page-map'
import { Logo } from './logo'
import 'nextra-theme-docs/style.css'
import './globals.css'

export const metadata = {
  // Origin used to resolve the generated OG image to an absolute URL (basePath
  // is appended automatically). Update this if a custom domain is configured.
  metadataBase: new URL('https://sainathr19.github.io'),
  title: {
    default: 'Wharfnet',
    template: '%s | Wharfnet'
  },
  description:
    'One-command localnet for EVM, Solana & Starknet — built-in faucet, pre-deployed test tokens and more.',
  openGraph: {
    title: 'Wharfnet',
    description:
      'One-command localnet for EVM, Solana & Starknet — built-in faucet, pre-deployed test tokens and more.',
    url: '/wharfnet',
    siteName: 'Wharfnet',
    type: 'website'
  },
  twitter: {
    card: 'summary_large_image',
    title: 'Wharfnet',
    description: 'One-command localnet for EVM, Solana & Starknet.'
  }
}

const navbar = (
  <Navbar logo={<Logo />} projectLink="https://github.com/sainathr19/wharfnet" />
)

const footer = (
  <Footer>
    MIT {new Date().getFullYear()} ©{' '}
    <a href="https://github.com/sainathr19/wharfnet" target="_blank" rel="noreferrer">
      Wharfnet
    </a>
  </Footer>
)

export default async function RootLayout({ children }) {
  return (
    <html lang="en" dir="ltr" suppressHydrationWarning>
      <Head color={{ hue: 200, saturation: 90, lightness: 55 }} />
      <body>
        <Layout
          navbar={navbar}
          footer={footer}
          pageMap={await getPageMap()}
          docsRepositoryBase="https://github.com/sainathr19/wharfnet/tree/main/landing"
          sidebar={{ defaultMenuCollapseLevel: 1 }}
        >
          {children}
        </Layout>
      </body>
    </html>
  )
}
