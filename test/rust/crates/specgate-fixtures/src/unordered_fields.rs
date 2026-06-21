// Fixture used by unordered_fields.spec.yaml — must expose make_account +
// withdraw with the same two-field Account so $unordered can match both
// balance and transaction_count events.
use specgate::*;

#[spec_setup("make_account")]
pub fn make_account() -> Account {
    Account { balance: 100, transaction_count: 0 }
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
