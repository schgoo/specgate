using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.MultiSetup;

/// <summary>
/// net8.0 copy of the multi-setup transfer fixture — the cross-framework
/// witness for static-method parameter mutation and prefixing on an older TFM.
/// </summary>
public class Account
{
    /// <summary>The account balance; each assignment is captured as a state mutation.</summary>
    [SpecEvent("balance")]
    public int Balance { get; set; }
}

/// <summary>
/// Transfer operation with two setups (pinned via
/// <see cref="SpecSetupAttribute.Fills"/>) building the <c>source</c> and
/// <c>target</c> accounts.
/// </summary>
public static class TransferOps
{
    /// <summary>Builds the source account with a starting balance of 100.</summary>
    /// <returns>A new <see cref="Account"/> for the <c>source</c> parameter.</returns>
    [SpecSetup("transfer", Fills = "source")]
    public static Account MakeSource() => new() { Balance = 100 };

    /// <summary>Builds the target account with a starting balance of 0.</summary>
    /// <returns>A new <see cref="Account"/> for the <c>target</c> parameter.</returns>
    [SpecSetup("transfer", Fills = "target")]
    public static Account MakeTarget() => new() { Balance = 0 };

    /// <summary>Moves <paramref name="amount"/> from <paramref name="source"/> to <paramref name="target"/>.</summary>
    /// <param name="source">The debited account (built by <see cref="MakeSource"/>).</param>
    /// <param name="target">The credited account (built by <see cref="MakeTarget"/>).</param>
    /// <param name="amount">The amount to transfer (spec input <c>amount</c>).</param>
    [SpecOperation("transfer")]
    public static void Transfer(Account source, Account target, [SpecInput("amount")] int amount)
    {
        source.Balance -= amount;
        target.Balance += amount;
    }
}
