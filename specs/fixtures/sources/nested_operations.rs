// Nested operations: transfer calls withdraw and deposit.
use specgate_annotations::*;

#[spec_setup("make_account")]
fn make_account() -> Account {
    Account { balance: 100 }
}

struct Account {
    #[spec_event]
    balance: i32,
}

impl Account {
    #[spec_operation("transfer")]
    fn transfer(&mut self, amount: i32) {
        self.withdraw(amount);
        self.deposit(amount);
    }

    #[spec_operation("withdraw")]
    fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
    }

    #[spec_operation("deposit")]
    fn deposit(&mut self, amount: i32) {
        self.balance += amount;
    }
}
