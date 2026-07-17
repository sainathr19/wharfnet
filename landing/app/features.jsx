// Landing-page feature highlights — icon + title + one-line description,
// laid out as a responsive grid. Imported by content/index.mdx.

const icons = {
  chains: (
    <path d="M9 17H7A5 5 0 0 1 7 7h2m6 0h2a5 5 0 0 1 0 10h-2M8 12h8" />
  ),
  battery: (
    <>
      <rect x="2" y="7" width="16" height="10" rx="2" />
      <line x1="22" y1="11" x2="22" y2="13" />
      <line x1="6" y1="12" x2="6" y2="12" />
      <line x1="10" y1="12" x2="10" y2="12" />
    </>
  ),
  repeat: <path d="M17 2l4 4-4 4M3 11V9a4 4 0 0 1 4-4h14M7 22l-4-4 4-4m14-1v2a4 4 0 0 1-4 4H3" />,
  layers: <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />,
  faucet: (
    <>
      <path d="M12 2v6" />
      <path d="M9 8h6l-1 4h-4z" />
      <path d="M12 12v4a3 3 0 0 1-3 3H6" />
      <circle cx="6" cy="21" r="1" />
    </>
  ),
  terminal: (
    <>
      <polyline points="4 17 10 11 4 5" />
      <line x1="12" y1="19" x2="20" y2="19" />
    </>
  ),
  beaker: (
    <>
      <path d="M9 3h6" />
      <path d="M10 3v6l-4.5 8A2 2 0 0 0 7.3 20h9.4a2 2 0 0 0 1.8-3l-4.5-8V3" />
      <line x1="7" y1="14" x2="17" y2="14" />
    </>
  )
}

function Icon({ name }) {
  return (
    <svg
      className="feature-icon"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {icons[name]}
    </svg>
  )
}

export function Feature({ icon, title, children }) {
  return (
    <div className="feature">
      <Icon name={icon} />
      <div className="feature-body">
        <span className="feature-title">{title}</span>
        {children ? <span className="feature-desc">{children}</span> : null}
      </div>
    </div>
  )
}

export function Features({ children }) {
  return <div className="feature-grid">{children}</div>
}
