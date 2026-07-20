using SpecGate.Annotations;

namespace SpecGateFixtures.Mocks.MockNotFound;

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
/// Service whose <c>get_user</c> operation calls a table-mocked dependency with
/// an input absent from the table, demonstrating the miss path (error event,
/// operation returns its default).
/// </summary>
public class UserService
{
    [SpecMock("db")]
    private readonly RealDb _db = new();

    /// <summary>Builds the service (with a real, never-invoked dependency).</summary>
    /// <returns>A new <see cref="UserService"/>.</returns>
    [SpecSetup("get_user")]
    public static UserService Make() => new();

    /// <summary>Returns the user record for <paramref name="id"/> via the mocked db.</summary>
    /// <param name="id">The user id (spec input <c>id</c>).</param>
    /// <returns>The db's response, or the default when the input is not in the table.</returns>
    [SpecOperation("get_user")]
    public string GetUser([SpecInput("id")] string id)
    {
        string response = _db.Find(id);
        return response;
    }
}
