import nextra from 'nextra'

const withNextra = nextra({
  defaultShowCopyCode: true,
  search: {
    codeblocks: false
  }
})

// GitHub Pages serves a project site under /<repo>, so production assets are
// prefixed with the repo path. Local dev serves from the root.
const isProd = process.env.NODE_ENV === 'production'
const repo = 'wharfnet'

export default withNextra({
  output: 'export',
  // Pin the workspace root to this folder so Next doesn't walk up to a parent
  // lockfile on machines that have one above the repo.
  outputFileTracingRoot: import.meta.dirname,
  images: { unoptimized: true },
  basePath: isProd ? `/${repo}` : '',
  // GitHub Pages runs Jekyll by default, which drops files/folders that start
  // with an underscore (Next.js emits `_next`); trailingSlash keeps URLs clean
  // and the workflow adds `.nojekyll` to disable Jekyll entirely.
  trailingSlash: true
})
