// Nested operations: transfer calls withdraw and deposit.
use specgate::*;

#[spec_setup("transfer")]
pub fn make_account() -> Account {
    Account { balance: 100 }
}

#[derive(SpecEvent)]
pub struct Account {
    #[spec_event]
    pub balance: i32,
}

impl Account {
    #[spec_operation("transfer", spec = "fixture.nested_operations")]
    pub fn transfer(&mut self, amount: i32) {
        self.withdraw(amount);
        self.deposit(amount);
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
