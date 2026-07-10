//! Rebasing ERC-20 — the Cairo analogue of `RebasingToken` in
//! `../src/WeirdTokens.sol`. Balances scale by a global multiplier that can change
//! at any time (like stETH/AMPL): a holder's `balance_of` moves with no transfer
//! to their account. Breaks code that caches balances or assumes they only change
//! on transfer. `mint` is public. Never deploy this to a real network.

use starknet::ContractAddress;

#[starknet::interface]
pub trait IRebasingToken<TContractState> {
    fn name(self: @TContractState) -> felt252;
    fn symbol(self: @TContractState) -> felt252;
    fn decimals(self: @TContractState) -> u8;
    fn total_supply(self: @TContractState) -> u256;
    fn balance_of(self: @TContractState, account: ContractAddress) -> u256;
    fn allowance(
        self: @TContractState, owner: ContractAddress, spender: ContractAddress,
    ) -> u256;
    fn transfer(ref self: TContractState, recipient: ContractAddress, amount: u256) -> bool;
    fn transfer_from(
        ref self: TContractState,
        sender: ContractAddress,
        recipient: ContractAddress,
        amount: u256,
    ) -> bool;
    fn approve(ref self: TContractState, spender: ContractAddress, amount: u256) -> bool;
    fn mint(ref self: TContractState, recipient: ContractAddress, amount: u256);
    /// Set the global multiplier (wad, 1e18 == 1.0); every holder's balance
    /// rescales instantly.
    fn rebase(ref self: TContractState, new_factor_wad: u256);
}

#[starknet::contract]
pub mod RebasingToken {
    use starknet::storage::{
        Map, StorageMapReadAccess, StorageMapWriteAccess, StoragePointerReadAccess,
        StoragePointerWriteAccess,
    };
    use starknet::{ContractAddress, get_caller_address};

    /// 1.0 in wad. balance = shares * rebase_factor / WAD.
    const WAD: u256 = 1000000000000000000;

    #[storage]
    struct Storage {
        rebase_factor: u256,
        total_shares: u256,
        shares: Map<ContractAddress, u256>,
        allowances: Map<(ContractAddress, ContractAddress), u256>,
    }

    #[event]
    #[derive(Drop, starknet::Event)]
    enum Event {
        Transfer: Transfer,
        Approval: Approval,
    }

    #[derive(Drop, starknet::Event)]
    struct Transfer {
        #[key]
        from: ContractAddress,
        #[key]
        to: ContractAddress,
        value: u256,
    }

    #[derive(Drop, starknet::Event)]
    struct Approval {
        #[key]
        owner: ContractAddress,
        #[key]
        spender: ContractAddress,
        value: u256,
    }

    #[constructor]
    fn constructor(ref self: ContractState) {
        self.rebase_factor.write(WAD);
    }

    #[abi(embed_v0)]
    impl RebasingTokenImpl of super::IRebasingToken<ContractState> {
        fn name(self: @ContractState) -> felt252 {
            'Rebasing Token'
        }
        fn symbol(self: @ContractState) -> felt252 {
            'REB'
        }
        fn decimals(self: @ContractState) -> u8 {
            18
        }
        fn total_supply(self: @ContractState) -> u256 {
            self.total_shares.read() * self.rebase_factor.read() / WAD
        }
        fn balance_of(self: @ContractState, account: ContractAddress) -> u256 {
            self.shares.read(account) * self.rebase_factor.read() / WAD
        }
        fn allowance(
            self: @ContractState, owner: ContractAddress, spender: ContractAddress,
        ) -> u256 {
            self.allowances.read((owner, spender))
        }

        fn transfer(ref self: ContractState, recipient: ContractAddress, amount: u256) -> bool {
            self._transfer(get_caller_address(), recipient, amount);
            true
        }

        fn transfer_from(
            ref self: ContractState,
            sender: ContractAddress,
            recipient: ContractAddress,
            amount: u256,
        ) -> bool {
            let caller = get_caller_address();
            let allowed = self.allowances.read((sender, caller));
            if allowed != core::num::traits::Bounded::MAX {
                self.allowances.write((sender, caller), allowed - amount);
            }
            self._transfer(sender, recipient, amount);
            true
        }

        fn approve(ref self: ContractState, spender: ContractAddress, amount: u256) -> bool {
            let owner = get_caller_address();
            self.allowances.write((owner, spender), amount);
            self.emit(Approval { owner, spender, value: amount });
            true
        }

        /// Public mint — credits shares worth `amount` at the current factor.
        fn mint(ref self: ContractState, recipient: ContractAddress, amount: u256) {
            let minted = amount * WAD / self.rebase_factor.read();
            self.shares.write(recipient, self.shares.read(recipient) + minted);
            self.total_shares.write(self.total_shares.read() + minted);
            self.emit(Transfer { from: zero(), to: recipient, value: amount });
        }

        fn rebase(ref self: ContractState, new_factor_wad: u256) {
            assert(new_factor_wad > 0, 'factor=0');
            self.rebase_factor.write(new_factor_wad);
        }
    }

    #[generate_trait]
    impl InternalImpl of InternalTrait {
        fn _transfer(
            ref self: ContractState,
            from: ContractAddress,
            to: ContractAddress,
            amount: u256,
        ) {
            let moved = amount * WAD / self.rebase_factor.read();
            self.shares.write(from, self.shares.read(from) - moved);
            self.shares.write(to, self.shares.read(to) + moved);
            self.emit(Transfer { from, to, value: amount });
        }
    }

    fn zero() -> ContractAddress {
        0.try_into().unwrap()
    }
}
