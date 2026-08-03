//! C# reflection-based structural discovery.
//!
//! The Rust discovery path ([`crate::discovery::run_discovery`]) links the
//! target crate and prints its link-time `discovery_json()`. The C# analog here
//! is a small generated program that is compiled together with the fixture
//! sources and the `SpecGate` runtime, then **reflects** over the resulting
//! assembly's `[SpecOperation]` / `[SpecSetup]` / `[SpecEvent]` /
//! `[SpecException]` metadata (never scanning C# source text) and prints the
//! same RAW registry JSON shape the Rust runtime emits. Types are normalized to
//! spec types inside the C# program — with full reflection fidelity, including
//! `System.Reflection.NullabilityInfoContext` for nullable reference types — so
//! the parsed registry folds through the shared, language-neutral setup-folding
//! path in [`crate::discovery`] to a `DiscoveredSchema` identical to the Rust
//! canonical.

use std::path::Path;
use std::process::Command;

/// Build and run the C# discovery program for `target`, scoped to `component`,
/// and return the raw registry JSON it prints (same shape as the Rust runtime's
/// `discovery_json()`: `{ "operations": [...], "types": [...] }`).
///
/// Scaffolds into the shared, component/framework-keyed discovery scratch dir.
/// Callers that may run concurrently against the same component (e.g. the C#
/// behavioral runner) must instead use [`run_csharp_discovery_in`] with a
/// caller-private scratch dir to avoid clobbering the same `Runner.dll`.
///
/// # Errors
///
/// Returns an error string when the scaffold, `dotnet` build/run, or output
/// read fails.
pub(crate) fn run_csharp_discovery(target: &crate::binding::Target, component: &str) -> Result<String, String> {
    let settings = crate::resolve_csharp_runner_settings(target);
    let sanitized_component: String = component.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let sanitized_framework: String = settings
        .framework
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let scratch = crate::discovery::workspace_root()
        .join("target")
        .join("specgate-discovery-cs")
        .join(format!("{sanitized_component}_{sanitized_framework}"));
    run_csharp_discovery_in(target, component, &scratch)
}

/// Build and run the C# reflection self-report for `target`/`component`,
/// scaffolding into `scratch`. Returns the raw registry JSON.
///
/// # Errors
///
/// Returns an error string when the scaffold, `dotnet` build/run, or output
/// read fails.
pub(crate) fn run_csharp_discovery_in(target: &crate::binding::Target, component: &str, scratch: &Path) -> Result<String, String> {
    let settings = crate::resolve_csharp_runner_settings(target);
    let pkg_abs = std::fs::canonicalize(&target.package_root).unwrap_or_else(|_| target.package_root.clone());
    let pkg_fwd = crate::path_to_forward_slash(&pkg_abs);

    std::fs::create_dir_all(scratch).map_err(|e| format!("failed to scaffold C# discovery dir: {e}"))?;

    let runtime_sources = runtime_compile_items(&pkg_abs);
    let lang_version = settings
        .lang_version
        .as_ref()
        .map(|v| format!("    <LangVersion>{}</LangVersion>\n", crate::escape_xml_text(v)))
        .unwrap_or_default();

    let csproj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    <TargetFramework>{framework}</TargetFramework>\n    \
         <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n    \
         <Nullable>{nullable}</Nullable>\n    <ImplicitUsings>{implicit_usings}</ImplicitUsings>\n{lang_version}  </PropertyGroup>\n  <ItemGroup>\n    \
         <Compile Include=\"Program.cs\" />\n    \
         <Compile Include=\"{pkg}/**/*.cs\" Exclude=\"{pkg}/Tests/**/*.cs;{pkg}/bin/**/*.cs;{pkg}/obj/**/*.cs\" LinkBase=\"Fixture\" />\n  \
         </ItemGroup>\n{runtime_sources}</Project>\n",
        framework = crate::escape_xml_text(&settings.framework),
        nullable = crate::escape_xml_text(&settings.nullable),
        implicit_usings = crate::escape_xml_text(&settings.implicit_usings),
        pkg = pkg_fwd,
    );
    std::fs::write(scratch.join("Runner.csproj"), csproj).map_err(|e| format!("failed to write C# discovery Runner.csproj: {e}"))?;

    let program = generate_csharp_discovery_program(component);
    std::fs::write(scratch.join("Program.cs"), program).map_err(|e| format!("failed to write C# discovery Program.cs: {e}"))?;

    let out_file = scratch.join("discovery.json");
    let _ = std::fs::remove_file(&out_file);

    let mut cmd = Command::new("dotnet");
    cmd.arg("run")
        .arg("--project")
        .arg(scratch.join("Runner.csproj"))
        .arg("--")
        .arg(&out_file)
        .current_dir(scratch);

    let output = cmd.output().map_err(|e| format!("failed to invoke dotnet for C# discovery: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stderr}\n{stdout}");
        return Err(format!(
            "C# discovery runner failed:\n{}",
            combined.lines().take(40).collect::<Vec<_>>().join("\n")
        ));
    }

    let json = std::fs::read_to_string(&out_file).map_err(|e| format!("C# discovery produced no output: {e}"))?;
    if json.trim().is_empty() {
        return Err("C# discovery produced empty output".to_string());
    }
    Ok(json)
}

/// Build the `<Compile>` items that pull the `SpecGate` runtime/annotation library
/// sources into the discovery runner, mirroring [`crate::run_csharp_group`] so
/// the annotation attribute types are available for reflection.
fn runtime_compile_items(package_root: &Path) -> String {
    let Some(libs) = crate::find_csharp_libs_dir(package_root) else {
        return String::new();
    };
    let mut lines: Vec<(String, String)> = Vec::new();
    for sub in ["SpecGate.Annotations", "SpecGate.Runtime"] {
        let dir = libs.join(sub);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("cs") {
                    let fwd = crate::path_to_forward_slash(&path);
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    lines.push((fwd, name));
                }
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    lines.sort_by(|a, b| a.1.cmp(&b.1));
    let items = lines
        .iter()
        .map(|(fwd, name)| format!("    <Compile Include=\"{fwd}\" Link=\"{name}\" />"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("  <ItemGroup>\n{items}\n  </ItemGroup>\n")
}

/// Render the discovery `Program.cs`: top-level statements that reflect over the
/// compiled assembly and emit the raw registry JSON for `component`.
fn generate_csharp_discovery_program(component: &str) -> String {
    let component_literal = crate::csharp_string_literal(component);
    CSHARP_DISCOVERY_PROGRAM.replace("__COMPONENT__", &component_literal)
}

/// The C# discovery program template. `__COMPONENT__` is replaced with the
/// target component name as a C# string literal.
const CSHARP_DISCOVERY_PROGRAM: &str = r#"using SpecGate.Annotations;
using SpecGate.Runtime;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Threading.Tasks;

const string Component = __COMPONENT__;

var ctx = new NullabilityInfoContext();
var operations = new List<object>();
var typeQueue = new List<Type>();
var seenTypes = new HashSet<Type>();
bool collectEnabled = true;

Type[] allTypes;
try
{
    allTypes = Assembly.GetExecutingAssembly().GetTypes();
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

bool HasSpecEvent(MemberInfo m) =>
    m.GetCustomAttributes(false).Any(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");

string? SpecEventName(MemberInfo m)
{
    object? attr = m.GetCustomAttributes(false)
        .FirstOrDefault(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");
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

// Render the raw C# type spelling (keywords for primitives, generic syntax with
// nullable annotations) that CODEGEN needs — the same spelling the retired
// source scanner recorded from the fixture text. This is distinct from MapCore,
// which normalizes to spec types for structural discovery.
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

foreach (Type type in allTypes)
{
    foreach (MethodInfo method in type.GetMethods(BindingFlags.Public | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly))
    {
        foreach (SpecOperationAttribute op in method.GetCustomAttributes<SpecOperationAttribute>())
        {
            if (op.Spec != Component) continue;
            NullabilityInfo retInfo = ctx.Create(method.ReturnParameter);
            var (isAsync, inner, innerInfo) = Unwrap(method.ReturnType, retInfo);
            bool hasException = method.GetCustomAttributes<SpecExceptionAttribute>().Any();
            string output;
            if (inner is null)
            {
                output = hasException ? "Result<value, string>" : "";
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
                return_type = output,
                fills = "",
                @params = BuildParams(method),
                component = Component,
                cs_class = (method.DeclaringType?.FullName ?? method.DeclaringType?.Name ?? "").Replace('+', '.'),
                cs_method_of = method.DeclaringType?.Name ?? "",
                cs_method = method.Name,
                cs_is_static = method.IsStatic,
                cs_return = MapRawCs(method.ReturnType, retInfo),
                cs_params = BuildRawParams(method),
                cs_exceptions = csExceptions,
            });
        }
        foreach (SpecSetupAttribute setup in method.GetCustomAttributes<SpecSetupAttribute>())
        {
            if (setup.Spec is not null && setup.Spec != Component) continue;
            NullabilityInfo retInfo = ctx.Create(method.ReturnParameter);
            var (_, inner, innerInfo) = Unwrap(method.ReturnType, retInfo);
            collectEnabled = false;
            string ret = inner is null ? "" : MapWithNull(inner, innerInfo);
            string csReturn = MapRawCs(method.ReturnType, retInfo);
            collectEnabled = true;
            operations.Add(new
            {
                name = setup.Name,
                is_setup = true,
                is_async = false,
                return_type = ret,
                fills = setup.Fills ?? "",
                @params = BuildParams(method),
                component = Component,
                cs_class = (method.DeclaringType?.FullName ?? method.DeclaringType?.Name ?? "").Replace('+', '.'),
                cs_method_of = method.DeclaringType?.Name ?? "",
                cs_method = method.Name,
                cs_is_static = method.IsStatic,
                cs_return = csReturn,
                cs_params = BuildRawParams(method),
                cs_exceptions = (object?)null,
            });
        }
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
File.WriteAllText(args[0], JsonSerializer.Serialize(payload));
"#;
