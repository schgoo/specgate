//! Generated C# reflection-runner source.

use super::project::string_literal;

/// Render the discovery `Program.cs`: top-level statements that reflect over the
/// compiled assembly once and emit one raw registry JSON document per requested
/// component.
pub(super) fn render_program(components: &[impl AsRef<str>]) -> String {
    let literals = components
        .iter()
        .map(|component| string_literal(component.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    PROGRAM.replace("__COMPONENTS__", &literals)
}

/// The C# discovery program template. `__COMPONENTS__` is replaced with the
/// requested component names as a comma-separated C# string literal list.
const PROGRAM: &str = r#"using SpecGate.Annotations;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;
using System.Threading.Tasks;

return DiscoveryRunner.Run(args, RunnerSystem.Real);

public static class DiscoveryRunner
{
public static int Run(string[] args, RunnerSystem runnerSystem)
{
string[] requestedComponents = new[] { __COMPONENTS__ };

// args[0] = output directory, args[1] = fixture assembly path, args[2] = the
// fixture's build output dir (holding its copy-local dependency assemblies).
string outputDirectory = args[0];
string fixtureDll = args[1];
string fixtureOut = args[2];

// Resolve the fixture's dependency assemblies from the build output dir so LoadFrom +
// GetTypes() + attribute reads + NullabilityInfoContext work against the real
// assembly and its references.
var dependencyResolver = runnerSystem.CreateResolver(fixtureDll);
using var resolverSubscription = runnerSystem.SubscribeResolver((ctx, name) =>
{
    string candidate = Path.Combine(fixtureOut, name.Name + ".dll");
    if (runnerSystem.FileExists(candidate)) return runnerSystem.LoadFromPath(ctx, candidate);
    string? resolved = runnerSystem.ResolveAssemblyPath(dependencyResolver, name);
    return resolved is not null ? runnerSystem.LoadFromPath(ctx, resolved) : null;
});

var ctx = new NullabilityInfoContext();
var operations = new List<object>();
var typeQueue = new List<Type>();
var seenTypes = new HashSet<Type>();
bool collectEnabled = true;

Assembly fixtureAssembly = runnerSystem.LoadAssembly(fixtureDll);
Type[] allTypes;
try
{
    allTypes = fixtureAssembly.GetTypes();
}
catch (ReflectionTypeLoadException ex)
{
    allTypes = ex.Types.Where(t => t is not null).Select(t => t!).ToArray();
}

void CollectType(Type t)
{
    if (collectEnabled && seenTypes.Add(t))
    {
        typeQueue.Add(t);
    }
}

// Coupled to the public annotation type emitted by SpecGate.Annotations.
const string SpecEventAttributeName = "SpecGate.Annotations.SpecEventAttribute";

bool HasSpecEvent(MemberInfo m) =>
    m.GetCustomAttributes(false).Any(a => a.GetType().FullName == SpecEventAttributeName);

string? SpecEventName(MemberInfo m)
{
    object? attr = m.GetCustomAttributes(false)
        .FirstOrDefault(a => a.GetType().FullName == SpecEventAttributeName);
    return attr?.GetType().GetProperty("Name")?.GetValue(attr) as string;
}

string MapCore(Type t, NullabilityInfo? info)
{
    if (t == typeof(object)) return "value";
    if (t == typeof(int)) return "i32";
    if (t == typeof(long)) return "i64";
    if (t == typeof(short)) return "i16";
    if (t == typeof(sbyte)) return "i8";
    if (t == typeof(double)) return "f64";
    if (t == typeof(float)) return "f32";
    if (t == typeof(bool)) return "bool";
    if (t == typeof(string)) return "string";
    if (t.IsArray)
    {
        Type elem = t.GetElementType()!;
        return "List<" + MapWithNull(elem, info?.ElementType) + ">";
    }
    if (t.IsGenericType)
    {
        Type def = t.GetGenericTypeDefinition();
        Type[] args = t.GetGenericArguments();
        NullabilityInfo[]? gtas = info?.GenericTypeArguments;
        NullabilityInfo? Ai(int i) => gtas is not null && i < gtas.Length ? gtas[i] : null;
        if (def == typeof(Option<>)) return "Option<" + MapWithNull(args[0], Ai(0)) + ">";
        if (def == typeof(Result<,>)) return "Result<" + MapWithNull(args[0], Ai(0)) + ", " + MapWithNull(args[1], Ai(1)) + ">";
        if (def == typeof(List<>)) return "List<" + MapWithNull(args[0], Ai(0)) + ">";
        if (def == typeof(Dictionary<,>) || def == typeof(SortedDictionary<,>))
            return "map<" + MapWithNull(args[0], Ai(0)) + ", " + MapWithNull(args[1], Ai(1)) + ">";
        if (def == typeof(HashSet<>) || def == typeof(SortedSet<>))
            return "set<" + MapWithNull(args[0], Ai(0)) + ">";
    }
    if (HasSpecEvent(t))
    {
        CollectType(t);
        return SpecEventName(t) ?? t.Name;
    }
    return t.Name;
}

string MapWithNull(Type t, NullabilityInfo? info)
{
    Type? underlying = Nullable.GetUnderlyingType(t);
    if (underlying is not null)
    {
        NullabilityInfo? inner = info?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
        return "Option<" + MapCore(underlying, inner) + ">";
    }
    if (info is not null && !t.IsValueType && info.ReadState == NullabilityState.Nullable)
    {
        return "Option<" + MapCore(t, info) + ">";
    }
    return MapCore(t, info);
}

// Render raw C# type spelling (keywords, generics, and nullable annotations)
// for future replay code generation. This is distinct from MapCore, which
// normalizes the language-neutral semantic surface.
string MapRawCsCore(Type t, NullabilityInfo? info)
{
    if (t == typeof(void)) return "void";
    if (t == typeof(object)) return "object";
    if (t == typeof(bool)) return "bool";
    if (t == typeof(string)) return "string";
    if (t == typeof(char)) return "char";
    if (t == typeof(int)) return "int";
    if (t == typeof(long)) return "long";
    if (t == typeof(short)) return "short";
    if (t == typeof(sbyte)) return "sbyte";
    if (t == typeof(byte)) return "byte";
    if (t == typeof(uint)) return "uint";
    if (t == typeof(ulong)) return "ulong";
    if (t == typeof(ushort)) return "ushort";
    if (t == typeof(double)) return "double";
    if (t == typeof(float)) return "float";
    if (t == typeof(decimal)) return "decimal";
    if (t.IsArray)
    {
        Type elem = t.GetElementType()!;
        return MapRawCs(elem, info?.ElementType) + "[]";
    }
    if (t.IsGenericType)
    {
        Type[] args = t.GetGenericArguments();
        NullabilityInfo[]? gtas = info?.GenericTypeArguments;
        NullabilityInfo? Ai(int i) => gtas is not null && i < gtas.Length ? gtas[i] : null;
        string baseName = t.Name;
        int tick = baseName.IndexOf('`');
        if (tick >= 0) baseName = baseName.Substring(0, tick);
        var parts = new List<string>();
        for (int i = 0; i < args.Length; i++) parts.Add(MapRawCs(args[i], Ai(i)));
        return baseName + "<" + string.Join(", ", parts) + ">";
    }
    return t.Name;
}

string MapRawCs(Type t, NullabilityInfo? info)
{
    Type? underlying = Nullable.GetUnderlyingType(t);
    if (underlying is not null)
    {
        NullabilityInfo? inner = info?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
        return MapRawCsCore(underlying, inner) + "?";
    }
    string core = MapRawCsCore(t, info);
    if (info is not null && !t.IsValueType && info.ReadState == NullabilityState.Nullable)
    {
        return core + "?";
    }
    return core;
}

List<string[]> BuildRawParams(MethodInfo m)
{
    var list = new List<string[]>();
    foreach (ParameterInfo p in m.GetParameters())
    {
        var inp = p.GetCustomAttribute<SpecInputAttribute>();
        string pname = inp?.Name ?? p.Name ?? "";
        string ptype = MapRawCs(p.ParameterType, ctx.Create(p));
        list.Add(new[] { pname, ptype });
    }
    return list;
}

(bool IsAsync, Type? Inner, NullabilityInfo? InnerInfo) Unwrap(Type ret, NullabilityInfo? retInfo)
{
    if (ret == typeof(void)) return (false, null, null);
    if (ret == typeof(Task) || ret == typeof(ValueTask)) return (true, null, null);
    if (ret.IsGenericType)
    {
        Type def = ret.GetGenericTypeDefinition();
        if (def == typeof(Task<>) || def == typeof(ValueTask<>))
        {
            Type inner = ret.GetGenericArguments()[0];
            NullabilityInfo? ii = retInfo?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
            return (true, inner, ii);
        }
    }
    return (false, ret, retInfo);
}

List<string[]> BuildParams(MethodInfo m)
{
    var list = new List<string[]>();
    foreach (ParameterInfo p in m.GetParameters())
    {
        var inp = p.GetCustomAttribute<SpecInputAttribute>();
        string pname = inp?.Name ?? p.Name ?? "";
        string ptype = MapWithNull(p.ParameterType, ctx.Create(p));
        list.Add(new[] { pname, ptype });
    }
    return list;
}

List<string[]> SpecMembers(Type t)
{
    var result = new List<string[]>();
    foreach (PropertyInfo pi in t.GetProperties(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly))
    {
        if (!HasSpecEvent(pi)) continue;
        result.Add(new[] { SpecEventName(pi) ?? pi.Name, MapWithNull(pi.PropertyType, ctx.Create(pi)) });
    }
    foreach (FieldInfo fi in t.GetFields(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly))
    {
        if (!HasSpecEvent(fi)) continue;
        result.Add(new[] { SpecEventName(fi) ?? fi.Name, MapWithNull(fi.FieldType, ctx.Create(fi)) });
    }
    return result;
}

runnerSystem.CreateDirectory(outputDirectory);

// The complete operation-component inventory of the compiled assembly, which
// is independent of what this run requested. Callers compare it against their
// own expected coverage, so an undeclared component cannot hide.
//
// Method reflection is deliberately Public|NonPublic: an annotation on a
// private method still declares a component, and hiding it here would let it
// escape both the inventory and the semantic validation that rejects a
// non-public operation. `is_public` below preserves the real accessibility, so
// normalization still rejects it by name. DeclaredOnly stays, so an inherited
// or compiler-generated member of a base type is never attributed to a
// derived one.
var annotatedMethods = allTypes
    .SelectMany(type => type.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly))
    .Select(method => new
    {
        method,
        operationAttributes = method.GetCustomAttributes<SpecOperationAttribute>().ToArray(),
        setupAttributes = method.GetCustomAttributes<SpecSetupAttribute>().ToArray(),
    })
    .Where(entry => entry.operationAttributes.Length != 0 || entry.setupAttributes.Length != 0)
    .ToArray();

var inventory = annotatedMethods
    .SelectMany(entry => entry.operationAttributes)
    .Select(operation => operation.Spec)
    .Where(spec => !string.IsNullOrEmpty(spec))
    .Select(spec => spec!)
    .Distinct(StringComparer.Ordinal)
    .OrderBy(spec => spec, StringComparer.Ordinal)
    .ToArray();
runnerSystem.WriteAllText(Path.Combine(outputDirectory, "components.json"), JsonSerializer.Serialize(inventory));

// Reflection spells nested CLR types with '+', while generated metadata is
// replayed as C# source names, whose nested separator is '.'.
string SourceTypeName(Type? type) => (type?.FullName ?? type?.Name ?? "").Replace('+', '.');

for (int componentIndex = 0; componentIndex < requestedComponents.Length; componentIndex++)
{
string Component = requestedComponents[componentIndex];
operations = new List<object>();
typeQueue = new List<Type>();
seenTypes = new HashSet<Type>();
collectEnabled = true;

var selectedOperationNames = annotatedMethods
    .SelectMany(entry => entry.operationAttributes)
    .Where(operation => operation.Spec == Component)
    .Select(operation => operation.Name)
    .ToHashSet(StringComparer.Ordinal);

foreach (var annotated in annotatedMethods)
{
        MethodInfo method = annotated.method;
        foreach (SpecOperationAttribute op in annotated.operationAttributes)
        {
            if (op.Spec != Component) continue;
            NullabilityInfo retInfo = ctx.Create(method.ReturnParameter);
            var (isAsync, inner, innerInfo) = Unwrap(method.ReturnType, retInfo);
            bool hasException = method.GetCustomAttributes<SpecExceptionAttribute>().Any();
            string output;
            if (inner is null)
            {
                output = hasException ? "Result<(), string>" : "";
            }
            else
            {
                string mapped = MapWithNull(inner, innerInfo);
                output = hasException ? "Result<" + mapped + ", string>" : mapped;
            }
            var exAttr = method.GetCustomAttribute<SpecExceptionAttribute>();
            object? csExceptions = exAttr is null ? null : (object)exAttr.ExceptionTypes.Select(et => et.Name).ToArray();
            operations.Add(new
            {
                name = op.Name,
                is_setup = false,
                is_async = isAsync,
                is_method = !method.IsStatic,
                is_public = method.IsPublic,
                return_type = output,
                fills = "",
                @params = BuildParams(method),
                component = Component,
                cs_class = SourceTypeName(method.DeclaringType),
                cs_method_of = method.DeclaringType?.Name ?? "",
                cs_method = method.Name,
                cs_is_static = method.IsStatic,
                cs_return = MapRawCs(method.ReturnType, retInfo),
                cs_params = BuildRawParams(method),
                cs_exceptions = csExceptions,
            });
        }
        foreach (SpecSetupAttribute setup in annotated.setupAttributes)
        {
            // An explicitly scoped setup belongs to exactly its own component.
            // An unscoped setup belongs to whichever component declares the
            // operation it prepares; without that guard it would leak into
            // every requested component's document.
            if (setup.Spec is not null)
            {
                if (setup.Spec != Component) continue;
            }
            else if (!selectedOperationNames.Contains(setup.Name)) continue;
            NullabilityInfo retInfo = ctx.Create(method.ReturnParameter);
            var (isAsync, inner, innerInfo) = Unwrap(method.ReturnType, retInfo);
            collectEnabled = false;
            string ret = inner is null ? "" : MapWithNull(inner, innerInfo);
            string csReturn = MapRawCs(method.ReturnType, retInfo);
            collectEnabled = true;
            if (selectedOperationNames.Contains(setup.Name) && inner is not null && HasSpecEvent(inner)) CollectType(inner);
            operations.Add(new
            {
                name = setup.Name,
                is_setup = true,
                is_async = isAsync,
                is_method = !method.IsStatic,
                is_public = method.IsPublic,
                return_type = ret,
                fills = setup.Fills ?? "",
                @params = BuildParams(method),
                component = Component,
                cs_class = SourceTypeName(method.DeclaringType),
                cs_method_of = method.DeclaringType?.Name ?? "",
                cs_method = method.Name,
                cs_is_static = method.IsStatic,
                cs_return = csReturn,
                cs_params = BuildRawParams(method),
                cs_exceptions = (object?)null,
            });
        }
}

var typeRecords = new List<object>();
for (int i = 0; i < typeQueue.Count; i++)
{
    Type t = typeQueue[i];
    string tname = SpecEventName(t) ?? t.Name;
    if (t.IsAbstract)
    {
        var variants = new List<object>();
        foreach (Type sub in allTypes.Where(x => x != t && !x.IsAbstract && t.IsAssignableFrom(x)))
        {
            variants.Add(new { name = SpecEventName(sub) ?? sub.Name, fields = SpecMembers(sub) });
        }
        typeRecords.Add(new { name = tname, kind = "enum", fields = new List<string[]>(), variants = variants, component = Component });
    }
    else
    {
        typeRecords.Add(new { name = tname, kind = "struct", fields = SpecMembers(t), variants = new List<object>(), component = Component });
    }
}

var payload = new { operations = operations, types = typeRecords };
// The Rust batch reader pairs each requested component with its zero-based
// position, so these positional filenames are part of the runner protocol.
const string COMPONENT_OUTPUT_EXTENSION = ".json";
runnerSystem.WriteAllText(Path.Combine(outputDirectory, componentIndex + COMPONENT_OUTPUT_EXTENSION), JsonSerializer.Serialize(payload));
}
return 0;
}
}

// Keep system-call orchestration behind one narrow adapter so reflection and
// payload construction do not directly depend on machine filesystem policy.
public sealed class RunnerSystem
{
    public static RunnerSystem Real { get; } = new(
        new FileSystemServices(File.Exists, path => Directory.CreateDirectory(path), File.WriteAllText),
        new AssemblyLoadingServices(
            path => Assembly.LoadFrom(path),
            (context, path) => context.LoadFromAssemblyPath(path)),
        new DependencyResolutionServices(
            path => new AssemblyDependencyResolver(path),
            (resolver, name) => resolver.ResolveAssemblyToPath(name),
            SubscribeDefaultResolver));

    public Func<string, bool> FileExists { get; }
    public Func<string, Assembly> LoadAssembly { get; }
    public Func<AssemblyLoadContext, string, Assembly> LoadFromPath { get; }
    public Func<string, AssemblyDependencyResolver> CreateResolver { get; }
    public Func<AssemblyDependencyResolver, AssemblyName, string?> ResolveAssemblyPath { get; }
    public Func<Func<AssemblyLoadContext, AssemblyName, Assembly?>, IDisposable> SubscribeResolver { get; }
    public Action<string> CreateDirectory { get; }
    public Action<string, string> WriteAllText { get; }

    public RunnerSystem(
        FileSystemServices files,
        AssemblyLoadingServices loading,
        DependencyResolutionServices resolution)
    {
        FileExists = files.FileExists;
        CreateDirectory = files.CreateDirectory;
        WriteAllText = files.WriteAllText;
        LoadAssembly = loading.LoadAssembly;
        LoadFromPath = loading.LoadFromPath;
        CreateResolver = resolution.CreateResolver;
        ResolveAssemblyPath = resolution.ResolveAssemblyPath;
        SubscribeResolver = resolution.SubscribeResolver;
    }

    private static IDisposable SubscribeDefaultResolver(Func<AssemblyLoadContext, AssemblyName, Assembly?> handler)
    {
        AssemblyLoadContext.Default.Resolving += handler;
        return new ResolverSubscription(handler);
    }

    private sealed class ResolverSubscription : IDisposable
    {
        private readonly Func<AssemblyLoadContext, AssemblyName, Assembly?> handler;
        public ResolverSubscription(Func<AssemblyLoadContext, AssemblyName, Assembly?> handler) => this.handler = handler;
        public void Dispose() => AssemblyLoadContext.Default.Resolving -= handler;
    }
}

public sealed record FileSystemServices(
    Func<string, bool> FileExists,
    Action<string> CreateDirectory,
    Action<string, string> WriteAllText);

public sealed record AssemblyLoadingServices(
    Func<string, Assembly> LoadAssembly,
    Func<AssemblyLoadContext, string, Assembly> LoadFromPath);

public sealed record DependencyResolutionServices(
    Func<string, AssemblyDependencyResolver> CreateResolver,
    Func<AssemblyDependencyResolver, AssemblyName, string?> ResolveAssemblyPath,
    Func<Func<AssemblyLoadContext, AssemblyName, Assembly?>, IDisposable> SubscribeResolver);
"#;
