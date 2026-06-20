// Multiple setup functions — setting up two objects for one operation.
use specgate_annotations::*;

#[spec_setup("make_source")]
pub fn make_source() -> Account {
    Account { balance: 100 }
}

#[spec_setup("make_target")]
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
