// Navbar brand: a small anchor mark (the "harbor" in wharfnet) + wordmark,
// mirroring the icon-plus-name lockup used by the reference docs site.
export function Logo() {
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: '0.5rem',
        fontWeight: 700
      }}
    >
      <svg
        width="22"
        height="22"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="5" r="2" />
        <line x1="12" y1="22" x2="12" y2="7" />
        <path d="M5 12H2a10 10 0 0 0 20 0h-3" />
        <line x1="8" y1="10" x2="16" y2="10" />
      </svg>
      <span>wharfnet</span>
    </span>
  )
}
