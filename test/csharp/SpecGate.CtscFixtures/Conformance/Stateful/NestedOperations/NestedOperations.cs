using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.NestedOperations;

[SpecEvent("Account")]
public sealed class Account
{
    [SpecEvent("balance")]
    public int Balance { get; set; }

    [SpecSetup("transfer", Spec = "fixture.nested_operations")]
    [SpecSetup("withdraw", Spec = "fixture.nested_operations")]
    [SpecSetup("deposit", Spec = "fixture.nested_operations")]
    public static Account Make() => new() { Balance = 100 };

    [SpecOperation("transfer", Spec = "fixture.nested_operations")]
    public void Transfer([SpecInput("amount")] int amount)
    {
        Withdraw(amount);
        Deposit(amount);
        _ = ObserveBalance(Balance);
    }

    [SpecOperation("observe_balance", Spec = "fixture.nested_operations")]
    public static int ObserveBalance([SpecInput("balance")] int balance) => balance;

    [SpecOperation("withdraw", Spec = "fixture.nested_operations")]
    public void Withdraw([SpecInput("amount")] int amount) => Balance -= amount;

    [SpecOperation("deposit", Spec = "fixture.nested_operations")]
    public void Deposit([SpecInput("amount")] int amount) => Balance += amount;
}
