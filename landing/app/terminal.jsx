// A faux terminal window for the landing page — shows a realistic `wharfnet up`
// + faucet session. Purely presentational.
export function BootDemo() {
  return (
    <div className="term">
      <div className="term-bar">
        <span className="term-dot" style={{ background: '#ff5f56' }} />
        <span className="term-dot" style={{ background: '#ffbd2e' }} />
        <span className="term-dot" style={{ background: '#27c93f' }} />
        <span className="term-title">zsh — wharfnet</span>
      </div>
      <pre className="term-body">
        <span className="t-dim">$ </span>wharfnet up{'\n'}
        <span className="t-accent">⚓</span> booting 7 chains + block explorers…{'\n'}
        {'  '}
        <span className="t-ok">✔</span> anvil-1{'     '}
        <span className="t-url">http://127.0.0.1:8545</span>
        {'      '}
        <span className="t-dim">evm</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> anvil-2{'     '}
        <span className="t-url">http://127.0.0.1:8546</span>
        {'      '}
        <span className="t-dim">evm</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> starknet-1{'  '}
        <span className="t-url">http://127.0.0.1:5050/rpc</span>
        {'  '}
        <span className="t-dim">starknet</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> solana-1{'    '}
        <span className="t-url">http://127.0.0.1:8899</span>
        {'      '}
        <span className="t-dim">solana</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> bitcoin-1{'   '}
        <span className="t-url">http://127.0.0.1:18443</span>
        {'     '}
        <span className="t-dim">bitcoin</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> litecoin-1{'  '}
        <span className="t-url">http://127.0.0.1:19443</span>
        {'     '}
        <span className="t-dim">litecoin</span>
        {'\n'}
        {'  '}
        <span className="t-ok">✔</span> zksync-1{'    '}
        <span className="t-url">http://127.0.0.1:8011</span>
        {'      '}
        <span className="t-dim">zksync</span>
        {'\n'}
        <span className="t-dim">  ready in 4.8s — funded dev accounts, test tokens, explorers</span>
        {'\n\n'}
        <span className="t-dim">$ </span>wharfnet faucet evm 0x70997970…79C8 1000 --token USDC{'\n'}
        {'  '}
        <span className="t-ok">✔</span> anvil-1: minted 1,000 USDC{'\n'}
        {'  '}
        <span className="t-ok">✔</span> anvil-2: minted 1,000 USDC
      </pre>
    </div>
  )
}
