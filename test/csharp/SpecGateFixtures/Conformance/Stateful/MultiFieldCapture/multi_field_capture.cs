using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.Stateful.MultiFieldCapture;

/// <summary>
/// State machine with multiple captured fields: <c>withdraw</c> mutates both
/// <c>balance</c> and <c>transaction_count</c>, each captured as state events.
/// The focused cross-language proof that every annotated field is captured, in
/// declaration order, before and after the operation.
/// </summary>
public class Account
{
    /// <summary>The running balance; captured as a state mutation.</summary>
    [SpecEvent("balance")]
    public int Balance { get; set; }

    /// <summary>The number of transactions; captured as a state mutation.</summary>
    [SpecEvent("transaction_count")]
    public int TransactionCount { get; set; }

    /// <summary>Builds an account with balance 100 and zero transactions.</summary>
    /// <returns>A new <see cref="Account"/>.</returns>
    [SpecSetup("withdraw", Spec = "fixture.multi_field_capture")]
    public static Account Make() => new() { Balance = 100, TransactionCount = 0 };

    /// <summary>Withdraws <paramref name="amount"/> and records the transaction.</summary>
    /// <param name="amount">The amount to withdraw (spec input <c>amount</c>).</param>
    [SpecOperation("withdraw", Spec = "fixture.multi_field_capture")]
    public void Withdraw([SpecInput("amount")] int amount)
    {
        Balance -= amount;
        TransactionCount += 1;
    }
}
