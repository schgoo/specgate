use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.nested_operations")]
pub struct Account {
    #[spec_event]
    pub balance: i32,
}

#[spec_setup("transfer", spec = "fixture.nested_operations")]
#[spec_setup("withdraw", spec = "fixture.nested_operations")]
#[spec_setup("deposit", spec = "fixture.nested_operations")]
pub fn make_account() -> Account {
    Account { balance: 100 }
}

impl Account {
    #[spec_operation("transfer", spec = "fixture.nested_operations")]
    pub fn transfer(&mut self, amount: i32) {
        self.withdraw(amount);
        self.deposit(amount);
        Self::observe_balance(self.balance);
    }

    #[spec_operation("observe_balance", spec = "fixture.nested_operations")]
    pub fn observe_balance(balance: i32) -> i32 {
        balance
    }

    #[spec_operation("withdraw", spec = "fixture.nested_operations")]
    pub fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
    }

    #[spec_operation("deposit", spec = "fixture.nested_operations")]
    pub fn deposit(&mut self, amount: i32) {
        self.balance += amount;
    }
}

#[test]
fn parent_operation_calls_two_annotated_child_operations() {
    let mut account = make_account();
    account.transfer(20);
    assert_eq!(account.balance, 100);
}
