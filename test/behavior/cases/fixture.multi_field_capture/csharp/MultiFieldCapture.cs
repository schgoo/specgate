using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.MultiFieldCapture;

[SpecEvent("Account")]
public sealed class Account
{
    [SpecEvent("balance")]
    public int Balance { get; set; }

    [SpecEvent("transaction_count")]
    public int TransactionCount { get; set; }

    [SpecSetup("withdraw", Spec = "fixture.multi_field_capture")]
    public static Account Make() => new() { Balance = 100 };

    [SpecOperation("withdraw", Spec = "fixture.multi_field_capture")]
    public void Withdraw([SpecInput("amount")] int amount)
    {
        Balance -= amount;
        TransactionCount++;
        _ = ObserveAccount(this);
    }

    [SpecOperation("observe_account", Spec = "fixture.multi_field_capture")]
    public static Account ObserveAccount([SpecInput("account")] Account account) =>
        new()
        {
            Balance = account.Balance,
            TransactionCount = account.TransactionCount,
        };
}
