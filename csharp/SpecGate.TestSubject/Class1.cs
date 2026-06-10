using SpecGate;

namespace SpecGate.TestSubject;

public class UserService
{
    private readonly IDatabase _db;

    [SpecInput("findUserByKey")]
    public UserService(IDatabase db)
    {
        _db = db;
    }

    [SpecOperation("findUserByKey", SpecKind.Pure)]
    public User FindByKey(string key)
    {
        return _db.GetUser(key);
    }

    [SpecEnvironment("findUserByKey")]
    public string TenantId { get; set; } = "";

    [SpecDependency("findUserByKey", Dep = "database")]
    public IDatabase Database => _db;
}

public interface IDatabase
{
    User GetUser(string key);
}

public record User(string Key, string Name);
