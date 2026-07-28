using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.Stateful.NestedOperations;

/// <summary>
/// Nested operations: <c>transfer</c> invokes the annotated <c>withdraw</c> and
/// <c>deposit</c> operations on itself. Those internal calls must emit their own
/// nested Run and input events so the trace matches the Rust target.
/// </summary>
public class Account
{
    /// <summary>The running balance; each assignment is captured as a state mutation.</summary>
    [SpecEvent("balance")]
    public int Balance { get; set; }

    /// <summary>Builds an account with a starting balance of 100.</summary>
    /// <returns>A new <see cref="Account"/> with <see cref="Balance"/> 100.</returns>
    [SpecSetup("transfer", Spec = "fixture.nested_operations")]
    public static Account Make() => new() { Balance = 100 };

    /// <summary>Transfers <paramref name="amount"/> by withdrawing then depositing it.</summary>
    /// <param name="amount">The amount to move (spec input <c>amount</c>).</param>
    [SpecOperation("transfer", Spec = "fixture.nested_operations")]
    public void Transfer([SpecInput("amount")] int amount)
    {
        Withdraw(amount);
        Deposit(amount);
    }

    /// <summary>Withdraws <paramref name="amount"/> from the balance.</summary>
    /// <param name="amount">The amount to withdraw (spec input <c>amount</c>).</param>
    [SpecOperation("withdraw", Spec = "fixture.nested_operations")]
    public void Withdraw([SpecInput("amount")] int amount) => Balance -= amount;

    /// <summary>Deposits <paramref name="amount"/> into the balance.</summary>
    /// <param name="amount">The amount to deposit (spec input <c>amount</c>).</param>
    [SpecOperation("deposit", Spec = "fixture.nested_operations")]
    public void Deposit([SpecInput("amount")] int amount) => Balance += amount;
}
