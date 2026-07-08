// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Deliberately non-standard ERC-20s for token-integration testing. Real tokens
// don't all follow the happy path — fee-on-transfer, rebasing, and
// missing-return-value tokens routinely break naive integrations. Each is
// pre-deployed on every wharfnet EVM chain at a fixed address so tests can point
// at known "weird" tokens instead of hand-rolling fixtures.
//
// Every token keeps a public, unpermissioned `mint(address,uint256)` so the
// wharfnet faucet can top up any address. Never deploy these to a real network.

/// @notice Takes a 1% fee on every transfer (the fee is burned), so the amount
///         received is less than the amount sent. Breaks code that assumes
///         `balanceAfter - balanceBefore == amountSent`.
contract FeeOnTransferToken {
    string public name = "Fee Token";
    string public symbol = "FEE";
    uint8 public constant decimals = 18;
    /// Fee taken on each transfer, in basis points (100 = 1%).
    uint256 public constant FEE_BPS = 100;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    /// Public mint — no fee is charged on mint, only on transfer.
    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
        emit Transfer(address(0), to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            allowance[from][msg.sender] = allowed - amount;
        }
        _transfer(from, to, amount);
        return true;
    }

    function _transfer(address from, address to, uint256 amount) internal {
        balanceOf[from] -= amount;
        uint256 fee = (amount * FEE_BPS) / 10_000;
        uint256 net = amount - fee;
        balanceOf[to] += net;
        totalSupply -= fee; // burn the fee
        emit Transfer(from, to, net);
        if (fee > 0) {
            emit Transfer(from, address(0), fee);
        }
    }
}

/// @notice Balances scale by a global multiplier that can change at any time
///         (like stETH/AMPL). A holder's `balanceOf` moves with no transfer to
///         their account. Breaks code that caches balances or assumes they only
///         change on transfer.
contract RebasingToken {
    string public name = "Rebasing Token";
    string public symbol = "REB";
    uint8 public constant decimals = 18;

    /// Multiplier in wad (1e18 == 1.0). balance = shares * rebaseFactor / 1e18.
    uint256 public rebaseFactor = 1e18;
    uint256 public totalShares;
    mapping(address => uint256) public sharesOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    function totalSupply() external view returns (uint256) {
        return (totalShares * rebaseFactor) / 1e18;
    }

    function balanceOf(address account) public view returns (uint256) {
        return (sharesOf[account] * rebaseFactor) / 1e18;
    }

    /// Set the global multiplier; every holder's balance rescales instantly.
    function rebase(uint256 newFactorWad) external {
        require(newFactorWad > 0, "factor=0");
        rebaseFactor = newFactorWad;
    }

    /// Public mint — credits shares worth `amount` at the current factor.
    function mint(address to, uint256 amount) external {
        uint256 shares = (amount * 1e18) / rebaseFactor;
        sharesOf[to] += shares;
        totalShares += shares;
        emit Transfer(address(0), to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            allowance[from][msg.sender] = allowed - amount;
        }
        _transfer(from, to, amount);
        return true;
    }

    function _transfer(address from, address to, uint256 amount) internal {
        uint256 shares = (amount * 1e18) / rebaseFactor;
        sharesOf[from] -= shares;
        sharesOf[to] += shares;
        emit Transfer(from, to, amount);
    }
}

/// @notice `transfer`/`approve`/`transferFrom` return nothing (like USDT on
///         mainnet). Breaks strict `require(token.transfer(...))` callers that
///         expect an ABI-decodable bool.
contract NoReturnToken {
    string public name = "No Return Token";
    string public symbol = "NRT";
    uint8 public constant decimals = 6;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
        emit Transfer(address(0), to, amount);
    }

    // Note: no bool return value — this is the whole point.
    function transfer(address to, uint256 amount) external {
        _transfer(msg.sender, to, amount);
    }

    function approve(address spender, uint256 amount) external {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
    }

    function transferFrom(address from, address to, uint256 amount) external {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            allowance[from][msg.sender] = allowed - amount;
        }
        _transfer(from, to, amount);
    }

    function _transfer(address from, address to, uint256 amount) internal {
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        emit Transfer(from, to, amount);
    }
}
