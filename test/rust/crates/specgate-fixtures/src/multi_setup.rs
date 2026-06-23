// Multiple setups constructing two receiver objects for one operation.
// Same output type (Account) → each setup pins itself with `fills`.
use specgate::*;

#[spec_setup("transfer", fills = "source")]
pub fn make_source() -> Account {
    Account { balance: 100 }
}

#[spec_setup("transfer", fills = "target")]
pub fn make_target() -> Account {
    Account { balance: 0 }
}

#[derive(SpecEvent)]
pub struct Account {
    #[spec_event]
    pub balance: i32,
}

#[spec_operation("transfer")]
pub fn transfer(source: &mut Account, target: &mut Account, amount: i32) {
    source.balance -= amount;
    target.balance += amount;
}
