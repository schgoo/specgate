use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.multi_field_capture")]
pub struct Account {
    #[spec_event]
    pub balance: i32,
    #[spec_event]
    pub transaction_count: i32,
}

#[spec_setup("withdraw", spec = "fixture.multi_field_capture")]
pub fn make_account() -> Account {
    Account {
        balance: 100,
        transaction_count: 0,
    }
}

impl Account {
    #[spec_operation("withdraw", spec = "fixture.multi_field_capture")]
    pub fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
        self.transaction_count += 1;
        observe_account(self);
    }
}

#[spec_operation("observe_account", spec = "fixture.multi_field_capture")]
pub fn observe_account(account: &Account) -> Account {
    account.clone()
}

#[test]
fn withdraw_mutates_all_captured_fields() {
    let mut account = make_account();
    account.withdraw(25);
    assert_eq!(
        account,
        Account {
            balance: 75,
            transaction_count: 1
        }
    );
}
