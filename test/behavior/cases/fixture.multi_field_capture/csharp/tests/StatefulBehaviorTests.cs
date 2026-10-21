using Xunit;
using MultiField = SpecGate.CtscFixtures.Conformance.Stateful.MultiFieldCapture;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    [Fact]
    public void MultiFieldAndRepeatedMutationPreserveIntermediateSemantics()
    {
        MultiField.Account account = MultiField.Account.Make();
        account.Withdraw(25);
        Assert.Equal(75, account.Balance);
        Assert.Equal(1, account.TransactionCount);
        Assert.Equal(75, MultiField.Account.ObserveAccount(account).Balance);
        MultiFieldAndRepeatedMutationPreserveIntermediateSemanticsStep1();
    }
}
