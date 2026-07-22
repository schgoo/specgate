//! Engine vehicle fixtures for the `fixture.matching` component — the
//! matcher / mismatch / subsequence / unordered behavior tests. It owns its own
//! `add`, `Counter`, and `Account` vehicle operations (all tagged
//! `spec = "fixture.matching"`) so those engine specs are self-contained and
//! never borrow a conformance fixture's operation. Each op belongs to exactly
//! one component, which is what lets `run_spec` resolve it unambiguously.
use specgate::*;

/// Stateless arithmetic vehicle for output/shape/subsequence matcher cases.
#[spec_operation("add", spec = "fixture.matching")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Single-field state vehicle for count-based matcher cases
/// (increment / decrement, including the multi-step case).
#[derive(SpecEvent)]
pub struct Counter {
    /// The running count.
    #[spec_event]
    pub count: i32,
}

/// Builds a fresh counter; `decrement` rides on the same receiver in step cases,
/// so a single `increment` setup is the one constructor for this type.
#[spec_setup("increment", spec = "fixture.matching")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

impl Counter {
    /// Adds one to the count.
    #[spec_operation("increment", spec = "fixture.matching")]
    pub fn increment(&mut self) {
        self.count += 1;
    }

    /// Subtracts one from the count.
    #[spec_operation("decrement", spec = "fixture.matching")]
    pub fn decrement(&mut self) {
        self.count -= 1;
    }
}

/// Separate single-field vehicle for the double-increment subsequence case.
/// It is a distinct type from `Counter` so its constructor does not collide
/// with `Counter`'s (a receiver type must have exactly one setup).
#[derive(SpecEvent)]
pub struct Doubler {
    /// The running count.
    #[spec_event]
    pub count: i32,
}

/// Builds a fresh doubler.
#[spec_setup("increment_twice", spec = "fixture.matching")]
pub fn make_doubler() -> Doubler {
    Doubler { count: 0 }
}

impl Doubler {
    /// Adds one twice, emitting an intermediate count for subsequence tests.
    #[spec_operation("increment_twice", spec = "fixture.matching")]
    pub fn increment_twice(&mut self) {
        self.count += 1;
        self.count += 1;
    }
}

/// Multi-field state vehicle for unordered / reordered field matcher cases.
#[derive(SpecEvent)]
pub struct Account {
    /// The remaining balance.
    #[spec_event]
    pub balance: i32,
    /// The number of transactions applied.
    #[spec_event]
    pub transaction_count: i32,
}

/// Builds a fresh account (balance 100) for withdraw cases.
#[spec_setup("withdraw", spec = "fixture.matching")]
pub fn make_account() -> Account {
    Account {
        balance: 100,
        transaction_count: 0,
    }
}

impl Account {
    /// Withdraws `amount`, updating balance and transaction count.
    #[spec_operation("withdraw", spec = "fixture.matching")]
    pub fn withdraw(&mut self, amount: i32) {
        self.balance -= amount;
        self.transaction_count += 1;
    }
}
