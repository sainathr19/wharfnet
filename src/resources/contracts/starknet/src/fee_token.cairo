//! Fee-on-transfer ERC-20 — the Cairo analogue of `FeeOnTransferToken` in
//! `../src/WeirdTokens.sol`. Takes a 1% fee on every transfer (the fee is
//! burned), so the amount received is less than the amount sent. Breaks code that
//! assumes `balance_after - balance_before == amount_sent`. `mint` is public and
//! charges no fee. Never deploy this to a real network.

use starknet::ContractAddress;

#[starknet::interface]
pub trait IFeeToken<TContractState> {
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
}

#[starknet::contract]
pub mod FeeToken {
    use starknet::storage::{
        Map, StorageMapReadAccess, StorageMapWriteAccess, StoragePointerReadAccess,
        StoragePointerWriteAccess,
    };
    use starknet::{ContractAddress, get_caller_address};

    /// Fee taken on each transfer, in basis points (100 = 1%).
    const FEE_BPS: u256 = 100;
    const BPS_DENOMINATOR: u256 = 10000;

    #[storage]
    struct Storage {
        total_supply: u256,
        balances: Map<ContractAddress, u256>,
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

    #[abi(embed_v0)]
    impl FeeTokenImpl of super::IFeeToken<ContractState> {
        fn name(self: @ContractState) -> felt252 {
            'Fee Token'
        }
        fn symbol(self: @ContractState) -> felt252 {
            'FEE'
        }
        fn decimals(self: @ContractState) -> u8 {
            18
        }
        fn total_supply(self: @ContractState) -> u256 {
            self.total_supply.read()
        }
        fn balance_of(self: @ContractState, account: ContractAddress) -> u256 {
            self.balances.read(account)
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

        /// Public mint — no fee is charged on mint, only on transfer.
        fn mint(ref self: ContractState, recipient: ContractAddress, amount: u256) {
            self.balances.write(recipient, self.balances.read(recipient) + amount);
            self.total_supply.write(self.total_supply.read() + amount);
            self.emit(Transfer { from: zero(), to: recipient, value: amount });
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
            self.balances.write(from, self.balances.read(from) - amount);
            let fee = amount * FEE_BPS / BPS_DENOMINATOR;
            let net = amount - fee;
            self.balances.write(to, self.balances.read(to) + net);
            self.total_supply.write(self.total_supply.read() - fee); // burn the fee
            self.emit(Transfer { from, to, value: net });
            if fee > 0 {
                self.emit(Transfer { from, to: zero(), value: fee });
            }
        }
    }

    fn zero() -> ContractAddress {
        0.try_into().unwrap()
    }
}
