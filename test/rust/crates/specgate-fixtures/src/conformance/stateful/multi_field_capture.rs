// StateMachine with multiple captured fields.
use specgate::*;

#[spec_setup("withdraw")]
pub fn make_account() -> Account {
    Account {
        balance: 100,
        transaction_count: 0,
    }
}

#[derive(SpecEvent)]
pub struct Account {
    #[spec_event]
    pub balance: i32,
    #[spec_event]
    pub transaction_count: i32,
}

impl Account {
    #[spec_operation("withdraw")]
    pub fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
        self.transaction_count += 1;
    }
}
