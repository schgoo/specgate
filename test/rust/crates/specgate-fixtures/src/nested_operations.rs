// Nested operations: transfer calls withdraw and deposit.
use specgate_annotations::*;

#[spec_setup("make_account")]
pub fn make_account() -> Account {
    Account { balance: 100 }
}

#[derive(SpecEvent)]
pub struct Account {
    #[spec_event]
    pub balance: i32,
}

impl Account {
    #[spec_operation("transfer")]
    pub fn transfer(&mut self, amount: i32) {
        self.withdraw(amount);
        self.deposit(amount);
    }

    #[spec_operation("withdraw")]
    pub fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
    }

    #[spec_operation("deposit")]
    pub fn deposit(&mut self, amount: i32) {
        self.balance += amount;
    }
}
