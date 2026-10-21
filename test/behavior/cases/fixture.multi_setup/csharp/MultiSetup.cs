using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.MultiSetup;

[SpecEvent("Account")]
public sealed class Account
{
    [SpecEvent("balance")]
    public int Balance { get; set; }
}

public static class Transfer
{
    [SpecSetup("transfer", Fills = "source", Spec = "fixture.multi_setup")]
    public static Account MakeSource() => new() { Balance = 100 };

    [SpecSetup("transfer", Fills = "target", Spec = "fixture.multi_setup")]
    public static Account MakeTarget() => new();

    [SpecOperation("transfer", Spec = "fixture.multi_setup")]
    public static void Execute(
        [SpecInput("source")] Account source,
        [SpecInput("target")] Account target,
        [SpecInput("amount")] int amount)
    {
        source.Balance -= amount;
        target.Balance += amount;
        _ = ObserveBalance("source", source);
        _ = ObserveBalance("target", target);
    }

    [SpecOperation("observe_balance", Spec = "fixture.multi_setup")]
    public static int ObserveBalance(
        [SpecInput("role")] string role,
        [SpecInput("account")] Account account)
    {
        ArgumentOutOfRangeException.ThrowIfNotEqual(role is "source" or "target", true);
        return account.Balance;
    }
}
