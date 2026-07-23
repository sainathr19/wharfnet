'use client'

import { useState } from 'react'
import { categories, entries } from '../content/changelog-data'

const CATEGORY_COLOR = {
  zkSync: '#5b6ef5',
  Solana: '#9945ff',
  Starknet: '#ec4899',
  EVM: '#3b82f6',
  UTXO: '#f7931a',
  Core: '#9ca3af'
}

// Split a plain segment into **bold** runs.
function renderBold(text, prefix) {
  return text.split('**').map((seg, i) =>
    i % 2 === 1 ? (
      <strong key={`${prefix}-${i}`}>{seg}</strong>
    ) : (
      <span key={`${prefix}-${i}`}>{seg}</span>
    )
  )
}

// Render a description string, turning `backtick` spans into <code> chips and
// **double-asterisk** spans into bold text.
function renderText(text) {
  return text.split('`').map((seg, i) =>
    i % 2 === 1 ? <code key={i}>{seg}</code> : <span key={i}>{renderBold(seg, i)}</span>
  )
}

export function Changelog() {
  const [active, setActive] = useState(() => new Set())

  const toggle = (cat) =>
    setActive((prev) => {
      const next = new Set(prev)
      next.has(cat) ? next.delete(cat) : next.add(cat)
      return next
    })

  const visible =
    active.size === 0 ? entries : entries.filter((e) => active.has(e.category))

  return (
    <div>
      <div className="cl-filters">
        <span className="cl-filters-label">Filter</span>
        {categories.map((cat) => {
          const on = active.has(cat)
          return (
            <button
              key={cat}
              className="cl-chip"
              data-active={on}
              onClick={() => toggle(cat)}
              style={on ? { '--cat': CATEGORY_COLOR[cat] } : undefined}
            >
              <span className="cl-chip-dot" style={{ background: CATEGORY_COLOR[cat] }} />
              {cat}
            </button>
          )
        })}
      </div>

      <div className="cl-timeline">
        {visible.map((e, i) => (
          <article className="cl-entry" key={i} style={{ '--cat': CATEGORY_COLOR[e.category] }}>
            <div className="cl-meta">
              <span className="cl-date">{e.date}</span>
              <span className="cl-cat">{e.category}</span>
              {e.tag ? <span className="cl-tag">{e.tag}</span> : null}
            </div>
            <div className="cl-body">
              <h3>{e.title}</h3>
              <ul className="cl-changes">
                {e.changes.map((c, j) => (
                  <li key={j}>{renderText(c)}</li>
                ))}
              </ul>
            </div>
          </article>
        ))}
        {visible.length === 0 ? (
          <p className="cl-empty">No entries match the selected filters.</p>
        ) : null}
      </div>
    </div>
  )
}
