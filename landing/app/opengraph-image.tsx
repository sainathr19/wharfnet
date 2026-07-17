import { ImageResponse } from 'next/og'

// Social preview card for link unfurls (og:image / twitter:image). Generated as
// a static PNG at build time.
export const alt = 'Wharfnet — one-command localnet for EVM, Solana & Starknet'
export const size = { width: 1200, height: 630 }
export const contentType = 'image/png'
// Required for `output: export` — generate the image once at build time.
export const dynamic = 'force-static'

export default function OpengraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          background: '#0a0a0b',
          backgroundImage:
            'radial-gradient(circle at 20% 15%, rgba(47,129,247,0.18), transparent 45%)',
          padding: '80px',
          color: '#fff',
          fontFamily: 'sans-serif'
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '20px' }}>
          <svg width="72" height="72" viewBox="0 0 24 24" fill="none" stroke="#2f81f7" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="5" r="2" />
            <line x1="12" y1="22" x2="12" y2="7" />
            <path d="M5 12H2a10 10 0 0 0 20 0h-3" />
            <line x1="8" y1="10" x2="16" y2="10" />
          </svg>
          <span style={{ fontSize: 64, fontWeight: 800, letterSpacing: '-0.02em' }}>Wharfnet</span>
        </div>

        <div style={{ marginTop: 32, fontSize: 40, fontWeight: 600, lineHeight: 1.25, maxWidth: 940 }}>
          One-command localnet for EVM, Solana &amp; Starknet
        </div>

        <div style={{ marginTop: 20, fontSize: 26, color: '#9aa0aa', maxWidth: 900 }}>
          Built-in faucet, pre-deployed test tokens, forking, and a block explorer per chain.
        </div>

        <div style={{ display: 'flex', gap: 16, marginTop: 48 }}>
          {['EVM', 'Solana', 'Starknet'].map((c) => (
            <div
              key={c}
              style={{
                display: 'flex',
                fontSize: 24,
                fontWeight: 600,
                padding: '10px 24px',
                borderRadius: 9999,
                border: '1px solid rgba(255,255,255,0.14)',
                background: 'rgba(255,255,255,0.04)'
              }}
            >
              {c}
            </div>
          ))}
        </div>
      </div>
    ),
    size
  )
}
