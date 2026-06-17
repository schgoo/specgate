// StateMachine with multiple captured fields.
use specgate_annotations::*;

#[spec_setup("make_account")]
fn make_account() -> Account {
    Account { balance: 100, transaction_count: 0 }
}

struct Account {
    #[spec_event]
    balance: i32,
    #[spec_event]
    transaction_count: i32,
}

impl Account {
    #[spec_operation("withdraw")]
    fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
        self.transaction_count += 1;
    }
}
