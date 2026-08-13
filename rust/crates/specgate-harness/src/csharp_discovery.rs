//! C# reflection-based structural discovery.
//!
//! The Rust discovery path ([`crate::discovery::run_discovery`]) links the
//! target crate and prints its link-time `discovery_json()`. The C# analog here
//! **builds the fixture's real project** into a per-run isolated artifacts tree
//! and then runs a small reflection program that loads the built fixture
//! assembly and **reflects** over its `[SpecOperation]` / `[SpecSetup]` /
//! `[SpecEvent]` / `[SpecException]` metadata (never scanning C# source text),
//! printing the same RAW registry JSON shape the Rust runtime emits. Building
//! the real assembly — rather than compiling a source-globbed surrogate — means
//! operations that exist only in the compiled output (e.g. those emitted by a
//! build-time source generator) are discovered too. Types are normalized to
//! spec types inside the C# program — with full reflection fidelity, including
//! `System.Reflection.NullabilityInfoContext` for nullable reference types — so
//! the parsed registry folds through the shared, language-neutral setup-folding
//! path in [`crate::discovery`] to a `DiscoveredSchema` identical to the Rust
//! canonical.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct CSharpBuildOutput {
    pub(crate) fixture_dll: PathBuf,
    pub(crate) fixture_out: PathBuf,
    pub(crate) assembly_name: String,
}

/// Build the fixture's real C# project into `scratch/artifacts` and return the
/// woven fixture assembly plus its copy-local output directory.
pub(crate) fn build_real_csharp_project(
    target: &crate::binding::Target,
    scratch: &Path,
    context: &str,
) -> Result<CSharpBuildOutput, String> {
    let pkg_abs = strip_verbatim_prefix(&std::fs::canonicalize(&target.package_root).unwrap_or_else(|_| target.package_root.clone()));
    std::fs::create_dir_all(scratch).map_err(|e| format!("failed to scaffold C# {context} dir: {e}"))?;

    let csproj = find_fixture_csproj(&pkg_abs).ok_or_else(|| format!("no .csproj found under {}", pkg_abs.display()))?;
    let project_name = csproj
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("could not read fixture .csproj file name")?
        .to_string();
    let csproj_text = std::fs::read_to_string(&csproj).map_err(|e| format!("failed to read fixture .csproj: {e}"))?;
    let assembly_name = crate::extract_csproj_xml_tag(&csproj_text, "AssemblyName").unwrap_or_else(|| project_name.clone());

    let artifacts_dir = scratch.join("artifacts");
    let mut build = Command::new("dotnet");
    build
        .arg("build")
        .arg(&csproj)
        .arg("-c")
        .arg("Debug")
        .arg("--artifacts-path")
        .arg(&artifacts_dir)
        .current_dir(scratch);
    let build_out = build
        .output()
        .map_err(|e| format!("failed to invoke dotnet build for C# {context}: {e}"))?;
    if !build_out.status.success() {
        let stderr = String::from_utf8_lossy(&build_out.stderr);
        let stdout = String::from_utf8_lossy(&build_out.stdout);
        let combined = format!("{stderr}\n{stdout}");
        return Err(format!(
            "C# {context} fixture build failed:\n{}",
            combined.lines().take(40).collect::<Vec<_>>().join("\n")
        ));
    }

    let fixture_out = artifacts_dir.join("bin").join(&project_name).join("debug");
    let fixture_dll = fixture_out.join(format!("{assembly_name}.dll"));
    if !fixture_dll.exists() {
        return Err(format!("C# {context} build produced no assembly at {}", fixture_dll.display()));
    }

    Ok(CSharpBuildOutput {
        fixture_dll,
        fixture_out,
        assembly_name,
    })
}

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
    // Keyed by component+framework (NOT per-invocation) on purpose: a fixture
    // with both a structural discover case and behavioral cases discovers the
    // same component more than once per run, and a stable scratch lets the
    // second call reuse the incremental build instead of a cold rebuild. Callers
    // never discover the SAME component+framework concurrently in the harness
    // (each conformance case is a distinct component, run sequentially), so the
    // shared scratch is safe there. The behavioral run path (which cannot cache)
    // uses a per-invocation pid/uid suffix instead. Do not run overlapping
    // same-component discovery concurrently (e.g. the ignored dev tests) without
    // adding per-invocation isolation.
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

    // 1. Build the fixture's REAL project into a per-run isolated artifacts tree,
    //    then reflect over the resulting assembly. This is what lets discovery
    //    observe operations that exist only in the compiled assembly (e.g. those
    //    emitted by a build-time source generator), which a source-globbing
    //    surrogate can never see. `--artifacts-path` lays every project in the
    //    build graph (the fixture + its referenced libraries) into its OWN
    //    `bin/<project>/<config>` and `obj/<project>/<config>` subfolders under
    //    the scratch dir, so concurrent harness runs never contend on — nor even
    //    touch — the source project's `obj`/`bin`.
    let built = build_real_csharp_project(target, scratch, "discovery")?;
    let fixture_dll = built.fixture_dll;
    let fixture_out = built.fixture_out;
    let fixture_out_fwd = crate::path_to_forward_slash(&fixture_out);

    // 2. Write a tiny reflection runner that references the SAME built
    //    SpecGate.Annotations / SpecGate.Runtime assemblies the fixture was
    //    compiled against (so attribute/generic type identity unifies under the
    //    default load context), loads the fixture assembly at runtime, and
    //    reflects over ITS types.
    let lang_version = settings
        .lang_version
        .as_ref()
        .map(|v| format!("    <LangVersion>{}</LangVersion>\n", crate::escape_xml_text(v)))
        .unwrap_or_default();

    let runner_csproj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    <TargetFramework>{framework}</TargetFramework>\n    \
         <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n    \
         <Nullable>{nullable}</Nullable>\n    <ImplicitUsings>{implicit_usings}</ImplicitUsings>\n{lang_version}  </PropertyGroup>\n  <ItemGroup>\n    \
         <Compile Include=\"Program.cs\" />\n  </ItemGroup>\n  <ItemGroup>\n    \
         <Reference Include=\"SpecGate.Annotations\">\n      <HintPath>{out}/SpecGate.Annotations.dll</HintPath>\n    </Reference>\n    \
         <Reference Include=\"SpecGate.Runtime\">\n      <HintPath>{out}/SpecGate.Runtime.dll</HintPath>\n    </Reference>\n  </ItemGroup>\n</Project>\n",
        framework = crate::escape_xml_text(&settings.framework),
        nullable = crate::escape_xml_text(&settings.nullable),
        implicit_usings = crate::escape_xml_text(&settings.implicit_usings),
        out = fixture_out_fwd,
    );
    std::fs::write(scratch.join("Runner.csproj"), runner_csproj).map_err(|e| format!("failed to write C# discovery Runner.csproj: {e}"))?;

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
        .arg(&fixture_dll)
        .arg(&fixture_out)
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

/// Find the first `.csproj` directly under `package_root` (the fixture project).
fn find_fixture_csproj(package_root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(package_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("csproj") {
            return Some(path);
        }
    }
    None
}

/// Strip a Windows verbatim (`\\?\`) prefix from `p`. `std::fs::canonicalize`
/// yields extended-length paths, which `MSBuild` mishandles when resolving
/// relative `<ProjectReference>` items and its default compile-item excludes
/// (leaving stale `obj/` sources globbed), so `dotnet` must be invoked with a
/// plain path.
fn strip_verbatim_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").map_or_else(|| p.to_path_buf(), PathBuf::from)
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
using System.Runtime.Loader;
using System.Text.Json;
using System.Threading.Tasks;

const string Component = __COMPONENT__;

// args[0] = output JSON path, args[1] = fixture assembly path, args[2] = the
// fixture's build output dir (holding its copy-local dependency assemblies).
string fixtureDll = args[1];
string fixtureOut = args[2];

// Resolve the fixture's dependency assemblies (SpecGate.Annotations,
// SpecGate.Runtime, YamlDotNet, …) from the build output dir so LoadFrom +
// GetTypes() + attribute reads + NullabilityInfoContext work against the real
// assembly and its references.
var dependencyResolver = new AssemblyDependencyResolver(fixtureDll);
AssemblyLoadContext.Default.Resolving += (ctx, name) =>
{
    string candidate = Path.Combine(fixtureOut, name.Name + ".dll");
    if (File.Exists(candidate)) return ctx.LoadFromAssemblyPath(candidate);
    string? resolved = dependencyResolver.ResolveAssemblyToPath(name);
    return resolved is not null ? ctx.LoadFromAssemblyPath(resolved) : null;
};

var ctx = new NullabilityInfoContext();
var operations = new List<object>();
var typeQueue = new List<Type>();
var seenTypes = new HashSet<Type>();
bool collectEnabled = true;

Assembly fixtureAssembly = Assembly.LoadFrom(fixtureDll);
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
