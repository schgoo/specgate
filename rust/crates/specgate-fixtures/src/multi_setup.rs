// Multiple setup functions — setting up two objects for one operation.
use specgate_annotations::*;

#[spec_setup("make_source")]
fn make_source() -> Account {
    Account { balance: 100 }
}

#[spec_setup("make_target")]
fn make_target() -> Account {
    Account { balance: 0 }
}

struct Account {
    #[spec_event]
    balance: i32,
}

#[spec_operation("transfer")]
fn transfer(source: &mut Account, target: &mut Account, amount: i32) {
    source.balance -= amount;
    target.balance += amount;
}
