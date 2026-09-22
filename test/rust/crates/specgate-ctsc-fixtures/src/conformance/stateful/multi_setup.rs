use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.multi_setup")]
pub struct Account {
    #[spec_event]
    pub balance: i32,
}

#[spec_setup("transfer", fills = "source", spec = "fixture.multi_setup")]
pub fn make_source() -> Account {
    Account { balance: 100 }
}

#[spec_setup("transfer", fills = "target", spec = "fixture.multi_setup")]
pub fn make_target() -> Account {
    Account { balance: 0 }
}

#[spec_operation("transfer", spec = "fixture.multi_setup")]
pub fn transfer(source: &mut Account, target: &mut Account, amount: i32) {
    source.balance -= amount;
    target.balance += amount;
    observe_balance("source", source);
    observe_balance("target", target);
}

#[spec_operation("observe_balance", spec = "fixture.multi_setup")]
pub fn observe_balance(role: &str, account: &Account) -> i32 {
    assert!(matches!(role, "source" | "target"));
    account.balance
}

#[test]
fn two_setups_fill_distinct_operation_parameters() {
    let mut source = make_source();
    let mut target = make_target();
    transfer(&mut source, &mut target, 30);
    assert_eq!(source.balance, 70);
    assert_eq!(target.balance, 30);
}
