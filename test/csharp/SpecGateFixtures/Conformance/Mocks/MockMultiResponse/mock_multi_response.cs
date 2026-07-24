using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Mocks.MockMultiResponse;

/// <summary>
/// A real database dependency that is never invoked under test — calls through
/// it are intercepted by the mock. Its methods throw to prove they are not run.
/// </summary>
public class RealDb
{
    /// <summary>Looks up a record by id. Never called under test (mocked).</summary>
    /// <param name="id">The record id.</param>
    /// <returns>The found value.</returns>
    public string Find(string id) => throw new InvalidOperationException("real db not available in test");
}

/// <summary>
/// Service whose operation makes two mocked calls, demonstrating that distinct
/// inputs resolve to distinct table responses within a single operation.
/// </summary>
public class UserService
{
    [SpecMock("db")]
    private readonly RealDb _db = new();

    /// <summary>Builds the service (with a real, never-invoked dependency).</summary>
    /// <returns>A new <see cref="UserService"/>.</returns>
    [SpecSetup("get_users", Spec = "fixture.mock_multi_response")]
    public static UserService Make() => new();

    /// <summary>Returns a combined description of two users via two mocked calls.</summary>
    /// <param name="idA">The first user id (spec input <c>id_a</c>).</param>
    /// <param name="idB">The second user id (spec input <c>id_b</c>).</param>
    /// <returns>The two db responses joined with <c>" and "</c>.</returns>
    [SpecOperation("get_users", Spec = "fixture.mock_multi_response")]
    public string GetTwoUsers([SpecInput("id_a")] string idA, [SpecInput("id_b")] string idB)
    {
        string a = _db.Find(idA);
        string b = _db.Find(idB);
        return $"{a} and {b}";
    }
}
