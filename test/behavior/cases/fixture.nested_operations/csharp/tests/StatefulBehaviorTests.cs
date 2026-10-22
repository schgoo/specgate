using Xunit;
using Nested = SpecGate.CtscFixtures.Conformance.Stateful.NestedOperations;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void MultiStepAndNestedOperationsShareReceiverStateStep1()
    {
        Nested.Account account = Nested.Account.Make();
        account.Transfer(20);
        Assert.Equal(100, account.Balance);
        account.Withdraw(15);
        Assert.Equal(85, account.Balance);
        account.Deposit(5);
        Assert.Equal(90, account.Balance);
    }
}
