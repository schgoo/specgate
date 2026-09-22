using System.Reflection;
using System.Text.Json;
using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.Basic;
using SpecGate.CtscFixtures.Registry.Focused.FallibleUnit;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class MetadataTests
{
    private static readonly JsonSerializerOptions JsonOptions =
        new() { PropertyNameCaseInsensitive = true };

    private static readonly string[] ComparableParityModes = ["byte-identical", "exception"];

    [Fact]
    public void GoldenMatrixOwnsTheDiscoverableCsharpComponentSet()
    {
        MatrixDocument matrix = LoadMatrix();
        List<MatrixRow> components =
            [.. matrix.Rows.Where(row => row.Classification == "implementation-component")];
        Assert.NotEmpty(components);

        HashSet<string> discovered = DiscoverComponents();
        IEnumerable<string> declared =
            components.Where(row => row.CSharp is not null).Select(row => row.Component!);

        Assert.Equal(declared.Order(), discovered.Order());

        foreach (MatrixRow row in components)
        {
            Assert.False(string.IsNullOrWhiteSpace(row.Component));
            if (row.CSharp is null)
            {
                Assert.Equal("rust-only", row.Parity.Mode);
                Assert.False(string.IsNullOrWhiteSpace(row.Parity.Reason));
                Assert.DoesNotContain(row.Component!, discovered);
                continue;
            }

            Assert.Contains(row.Parity.Mode, ComparableParityModes);
            if (row.Parity.Mode == "exception")
            {
                Assert.False(string.IsNullOrWhiteSpace(row.Parity.Reason));
            }
            else
            {
                Assert.Null(row.Parity.Reason);
            }
        }
    }

    [Fact]
    public void DiscoveryMetadataUsesStableExplicitNames()
    {
        Assembly assembly = typeof(StatelessAdd).Assembly;
        Dictionary<string, HashSet<string>> componentsByOperation =
            ComponentsByOperationName(assembly);

        foreach (MethodInfo method in AnnotatedMethods(assembly))
        {
            SpecOperationAttribute[] operations =
                [.. method.GetCustomAttributes<SpecOperationAttribute>()];
            SpecSetupAttribute[] setups =
                [.. method.GetCustomAttributes<SpecSetupAttribute>()];
            if (operations.Length == 0 && setups.Length == 0)
            {
                continue;
            }

            Assert.True(method.IsPublic);
            Assert.All(
                method.GetParameters(),
                parameter => Assert.Single(parameter.GetCustomAttributes<SpecInputAttribute>()));

            // An operation declares the component, so its Spec is the only
            // thing that can name one and must always be explicit.
            Assert.All(operations, operation => Assert.False(string.IsNullOrWhiteSpace(operation.Spec)));

            // A setup names the operation it prepares, so it may omit Spec --
            // but only while that operation name resolves to exactly one
            // component in the whole assembly. A second component declaring the
            // same operation name makes the omission ambiguous and fails here.
            foreach (SpecSetupAttribute setup in setups)
            {
                Assert.False(string.IsNullOrWhiteSpace(setup.Name));
                if (!string.IsNullOrWhiteSpace(setup.Spec))
                {
                    continue;
                }

                Assert.True(
                    componentsByOperation.TryGetValue(setup.Name, out HashSet<string>? owners),
                    $"unscoped setup '{setup.Name}' prepares no declared operation");
                Assert.Single(owners!);
            }
        }

        foreach (Type type in assembly.GetTypes())
        {
            SpecEventAttribute? typeEvent = type.GetCustomAttribute<SpecEventAttribute>();
            if (typeEvent is null)
            {
                continue;
            }

            Assert.False(string.IsNullOrWhiteSpace(typeEvent.Name));
            IEnumerable<MemberInfo> members =
                type.GetMembers(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                    .Where(
                        member =>
                            member.MemberType is MemberTypes.Field or MemberTypes.Property);
            foreach (MemberInfo member in members)
            {
                SpecEventAttribute? memberEvent = member.GetCustomAttribute<SpecEventAttribute>();
                Assert.NotNull(memberEvent);
                Assert.False(string.IsNullOrWhiteSpace(memberEvent!.Name));
            }
        }
    }

    /// <summary>
    /// The corpus keeps one real unscoped setup, so the supported fallback --
    /// resolving a setup through the sole component that declares its operation
    /// -- stays exercised rather than only documented.
    /// </summary>
    [Fact]
    public void AnUnscopedSetupResolvesThroughItsOperationsSoleComponent()
    {
        Assembly assembly = typeof(StatelessAdd).Assembly;
        MethodInfo make = typeof(UnitCounter).GetMethod(
            nameof(UnitCounter.Make),
            BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly)!;
        SpecSetupAttribute setup = Assert.Single(make.GetCustomAttributes<SpecSetupAttribute>());

        Assert.Equal("advance", setup.Name);
        Assert.True(string.IsNullOrWhiteSpace(setup.Spec), "the fixture must keep an unscoped setup");

        HashSet<string> owners = ComponentsByOperationName(assembly)[setup.Name];
        Assert.Equal("fixture.fallible_unit", Assert.Single(owners));

        SpecSetupAttribute[] unscoped =
            [..
            AnnotatedMethods(assembly)
                .SelectMany(method => method.GetCustomAttributes<SpecSetupAttribute>())
                .Where(candidate => string.IsNullOrWhiteSpace(candidate.Spec))];
        Assert.Contains(setup, unscoped);
    }

    private static MatrixDocument LoadMatrix() =>
        JsonSerializer.Deserialize<MatrixDocument>(
            File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "matrix.json")),
            JsonOptions)!;

    private static HashSet<string> DiscoverComponents() =>
        [..
        AnnotatedMethods(typeof(StatelessAdd).Assembly)
            .SelectMany(method => method.GetCustomAttributes<SpecOperationAttribute>())
            .Select(operation => operation.Spec!)];

    /// <summary>
    /// Every declared method of the fixture assembly, mirroring discovery's own
    /// reflection: non-public methods are included so an annotation can never
    /// hide behind accessibility, and DeclaredOnly keeps inherited members from
    /// being re-attributed to a derived type.
    /// </summary>
    private static IEnumerable<MethodInfo> AnnotatedMethods(Assembly assembly) =>
        assembly.GetTypes().SelectMany(
            type =>
                type.GetMethods(
                    BindingFlags.Public
                    | BindingFlags.NonPublic
                    | BindingFlags.Static
                    | BindingFlags.Instance
                    | BindingFlags.DeclaredOnly));

    /// <summary>
    /// The components that declare each operation name, which is what an
    /// unscoped setup resolves through.
    /// </summary>
    private static Dictionary<string, HashSet<string>> ComponentsByOperationName(Assembly assembly)
    {
        Dictionary<string, HashSet<string>> owners = [];
        foreach (SpecOperationAttribute operation in
            AnnotatedMethods(assembly).SelectMany(method => method.GetCustomAttributes<SpecOperationAttribute>()))
        {
            if (string.IsNullOrWhiteSpace(operation.Spec))
            {
                continue;
            }

            if (!owners.TryGetValue(operation.Name, out HashSet<string>? components))
            {
                components = new HashSet<string>(StringComparer.Ordinal);
                owners[operation.Name] = components;
            }

            components.Add(operation.Spec!);
        }

        return owners;
    }

    private sealed record MatrixDocument(List<MatrixRow> Rows);

    private sealed record MatrixRow(
        string Id,
        string Classification,
        string? Component,
        MatrixLanguage? CSharp,
        MatrixParity Parity);

    private sealed record MatrixLanguage(string Binding, List<string> Phases, string Expect);

    private sealed record MatrixParity(string Mode, string? Reason);
}
