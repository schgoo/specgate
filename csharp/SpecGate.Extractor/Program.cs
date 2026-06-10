using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Build.Locator;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.MSBuild;

MSBuildLocator.RegisterDefaults();

if (args.Length < 1)
{
    Console.Error.WriteLine("Usage: SpecGate.Extractor <path-to.csproj> [--output <file.json>]");
    return 1;
}

var projectPath = Path.GetFullPath(args[0]);
string? outputPath = null;
for (int i = 1; i < args.Length - 1; i++)
{
    if (args[i] == "--output") outputPath = args[i + 1];
}

if (!File.Exists(projectPath))
{
    Console.Error.WriteLine($"Project not found: {projectPath}");
    return 1;
}

Console.Error.WriteLine($"Loading project: {projectPath}");

using var workspace = MSBuildWorkspace.Create();
workspace.WorkspaceFailed += (_, e) =>
{
    if (e.Diagnostic.Kind == WorkspaceDiagnosticKind.Failure)
        Console.Error.WriteLine($"  workspace: {e.Diagnostic.Message}");
};

var project = await workspace.OpenProjectAsync(projectPath);
var compilation = await project.GetCompilationAsync();
if (compilation is null)
{
    Console.Error.WriteLine("Failed to get compilation.");
    return 1;
}

var specAttributes = new HashSet<string>
{
    "SpecOperation", "SpecOperationAttribute",
    "SpecInput", "SpecInputAttribute",
    "SpecCheckpoint", "SpecCheckpointAttribute",
    "SpecState", "SpecStateAttribute",
    "SpecEnvironment", "SpecEnvironmentAttribute",
    "SpecDependency", "SpecDependencyAttribute",
    "SpecGenerator", "SpecGeneratorAttribute",
    "SpecContext", "SpecContextAttribute",
};

var symbols = new List<AnnotatedSymbolDto>();
var referencedTypes = new HashSet<string>();

foreach (var tree in compilation.SyntaxTrees)
{
    var model = compilation.GetSemanticModel(tree);
    var root = await tree.GetRootAsync();
    var projectDir = Path.GetDirectoryName(projectPath) ?? "";

    foreach (var node in root.DescendantNodes())
    {
        ISymbol? symbol = node switch
        {
            MethodDeclarationSyntax m => model.GetDeclaredSymbol(m),
            ConstructorDeclarationSyntax c => model.GetDeclaredSymbol(c),
            PropertyDeclarationSyntax p => model.GetDeclaredSymbol(p),
            FieldDeclarationSyntax f when f.Declaration.Variables.Count == 1
                => model.GetDeclaredSymbol(f.Declaration.Variables[0]),
            _ => null,
        };

        if (symbol is null) continue;

        var attrs = symbol.GetAttributes()
            .Where(a => a.AttributeClass is not null &&
                        specAttributes.Contains(a.AttributeClass.Name))
            .ToList();

        if (attrs.Count == 0) continue;

        var dto = new AnnotatedSymbolDto
        {
            Name = symbol.Name == ".ctor" ? ".ctor" : symbol.Name,
            SymbolKind = GetSymbolKind(symbol),
            DeclaringType = symbol.ContainingType?.ToDisplayString() ?? "",
            Annotations = attrs.Select(ToAnnotationDto).ToList(),
        };

        // Location
        var loc = symbol.Locations.FirstOrDefault();
        if (loc?.IsInSource == true)
        {
            var lineSpan = loc.GetLineSpan();
            var filePath = Path.GetRelativePath(projectDir, lineSpan.Path);
            dto.Location = new SourceLocationDto
            {
                File = filePath,
                Line = lineSpan.StartLinePosition.Line + 1,
                Column = lineSpan.StartLinePosition.Character + 1,
            };
        }

        // Type info per symbol kind
        switch (symbol)
        {
            case IMethodSymbol method:
                dto.ReturnType = method.ReturnType.ToDisplayString();
                dto.IsStatic = method.IsStatic;
                dto.IsAsync = method.IsAsync;
                dto.Accessibility = MapAccessibility(method.DeclaredAccessibility);
                dto.Parameters = method.Parameters.Select(p => new ParameterDto
                {
                    Name = p.Name,
                    Type = p.Type.ToDisplayString(),
                    IsOptional = p.HasExplicitDefaultValue,
                    DefaultValue = p.HasExplicitDefaultValue ? p.ExplicitDefaultValue?.ToString() : null,
                }).ToList();

                // Track referenced types
                referencedTypes.Add(method.ReturnType.ToDisplayString());
                foreach (var p in method.Parameters)
                    referencedTypes.Add(p.Type.ToDisplayString());
                break;

            case IPropertySymbol prop:
                dto.Type = prop.Type.ToDisplayString();
                dto.IsStatic = prop.IsStatic;
                dto.Accessibility = MapAccessibility(prop.DeclaredAccessibility);
                referencedTypes.Add(prop.Type.ToDisplayString());
                break;

            case IFieldSymbol field:
                dto.Type = field.Type.ToDisplayString();
                dto.IsStatic = field.IsStatic;
                dto.Accessibility = MapAccessibility(field.DeclaredAccessibility);
                referencedTypes.Add(field.Type.ToDisplayString());
                break;
        }

        symbols.Add(dto);
    }
}

// Gather type info for all referenced types
var typeInfos = new List<TypeInfoDto>();
foreach (var typeName in referencedTypes)
{
    var typeSymbol = compilation.GetTypeByMetadataName(typeName);
    if (typeSymbol is null) continue;

    var ti = new TypeInfoDto
    {
        Name = typeSymbol.ToDisplayString(),
        IsAbstract = typeSymbol.IsAbstract,
        IsGeneric = typeSymbol.IsGenericType,
        GenericParameters = typeSymbol.TypeParameters.Select(tp => tp.Name).ToList(),
        Accessibility = MapAccessibility(typeSymbol.DeclaredAccessibility),
        BaseType = typeSymbol.BaseType?.ToDisplayString(),
    };

    // Check for SpecGenerator
    ti.HasSpecGenerator = symbols.Any(s =>
        s.Annotations.Any(a =>
            a.Attribute == "SpecGenerator" &&
            a.Args?.TypeName == typeSymbol.ToDisplayString()));

    // Constructors
    foreach (var ctor in typeSymbol.InstanceConstructors)
    {
        if (ctor.IsImplicitlyDeclared && ctor.Parameters.Length == 0) continue;
        ti.Constructors.Add(new ConstructorDto
        {
            Accessibility = MapAccessibility(ctor.DeclaredAccessibility),
            Parameters = ctor.Parameters.Select(p => new ParameterDto
            {
                Name = p.Name,
                Type = p.Type.ToDisplayString(),
                IsOptional = p.HasExplicitDefaultValue,
                DefaultValue = p.HasExplicitDefaultValue ? p.ExplicitDefaultValue?.ToString() : null,
            }).ToList(),
        });
    }

    typeInfos.Add(ti);
}

var result = new ExtractionResultDto
{
    SourceLanguage = "csharp",
    Project = Path.GetFileNameWithoutExtension(projectPath),
    Symbols = symbols,
    Types = typeInfos,
};

var jsonOpts = new JsonSerializerOptions
{
    WriteIndented = true,
    PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
};

var json = JsonSerializer.Serialize(result, jsonOpts);

if (outputPath is not null)
{
    File.WriteAllText(outputPath, json);
    Console.Error.WriteLine($"Wrote extraction to {outputPath}");
}
else
{
    Console.WriteLine(json);
}

Console.Error.WriteLine($"Extracted {symbols.Count} annotated symbol(s), {typeInfos.Count} type(s)");
return 0;

// ── Helpers ──

static string GetSymbolKind(ISymbol symbol) => symbol switch
{
    IMethodSymbol { MethodKind: MethodKind.Constructor } => "constructor",
    IMethodSymbol => "method",
    IPropertySymbol => "property",
    IFieldSymbol => "field",
    _ => "unknown",
};

static string MapAccessibility(Accessibility a) => a switch
{
    Microsoft.CodeAnalysis.Accessibility.Public => "public",
    Microsoft.CodeAnalysis.Accessibility.Internal => "internal",
    Microsoft.CodeAnalysis.Accessibility.Protected => "protected",
    Microsoft.CodeAnalysis.Accessibility.Private => "private",
    Microsoft.CodeAnalysis.Accessibility.ProtectedOrInternal => "internal",
    Microsoft.CodeAnalysis.Accessibility.ProtectedAndInternal => "protected",
    _ => "private",
};

static AnnotationDto ToAnnotationDto(AttributeData attr)
{
    var name = attr.AttributeClass!.Name.Replace("Attribute", "");
    var dto = new AnnotationDto { Attribute = name, Args = new AnnotationArgsDto() };

    // Positional args
    if (attr.ConstructorArguments.Length > 0)
    {
        var first = attr.ConstructorArguments[0];
        if (first.Value is string s)
            dto.Args.Name = s;
        else if (first.Type?.Name == "SpecKind")
            dto.Args.Kind = ResolveEnumName(first);
    }
    if (attr.ConstructorArguments.Length > 1)
    {
        var second = attr.ConstructorArguments[1];
        if (second.Type?.Name == "SpecKind")
            dto.Args.Kind = ResolveEnumName(second);
    }

    // Named args
    foreach (var narg in attr.NamedArguments)
    {
        switch (narg.Key)
        {
            case "Dep": dto.Args.Dep = narg.Value.Value?.ToString(); break;
            case "Kind": dto.Args.Kind = ResolveEnumName(narg.Value); break;
            case "TypeName": dto.Args.TypeName = narg.Value.Value?.ToString(); break;
        }
    }

    return dto;
}

static string? ResolveEnumName(TypedConstant tc)
{
    if (tc.Type is INamedTypeSymbol enumType && tc.Value is int intVal)
    {
        foreach (var member in enumType.GetMembers().OfType<IFieldSymbol>())
        {
            if (member.HasConstantValue && member.ConstantValue is int cv && cv == intVal)
                return member.Name;
        }
    }
    return tc.Value?.ToString();
}

// ── DTOs ──

class ExtractionResultDto
{
    public string SourceLanguage { get; set; } = "";
    public string? Project { get; set; }
    public List<AnnotatedSymbolDto> Symbols { get; set; } = [];
    public List<TypeInfoDto> Types { get; set; } = [];
}

class AnnotatedSymbolDto
{
    public string Name { get; set; } = "";
    public string SymbolKind { get; set; } = "";
    public string? DeclaringType { get; set; }
    public string? ReturnType { get; set; }
    public string? Type { get; set; }
    public List<ParameterDto>? Parameters { get; set; }
    public string? Accessibility { get; set; }
    public bool? IsStatic { get; set; }
    public bool? IsAsync { get; set; }
    public SourceLocationDto? Location { get; set; }
    public List<AnnotationDto> Annotations { get; set; } = [];
}

class AnnotationDto
{
    public string Attribute { get; set; } = "";
    public AnnotationArgsDto? Args { get; set; }
}

class AnnotationArgsDto
{
    public string? Name { get; set; }
    public string? Kind { get; set; }
    public string? Dep { get; set; }
    public string? TypeName { get; set; }
}

class TypeInfoDto
{
    public string Name { get; set; } = "";
    public bool? IsAbstract { get; set; }
    public bool? IsGeneric { get; set; }
    public List<string>? GenericParameters { get; set; }
    public string? Accessibility { get; set; }
    public List<ConstructorDto> Constructors { get; set; } = [];
    public bool? HasSpecGenerator { get; set; }
    public string? BaseType { get; set; }
}

class ConstructorDto
{
    public string? Accessibility { get; set; }
    public List<ParameterDto> Parameters { get; set; } = [];
}

class ParameterDto
{
    public string Name { get; set; } = "";
    public string Type { get; set; } = "";
    public bool? IsOptional { get; set; }
    public string? DefaultValue { get; set; }
}

class SourceLocationDto
{
    public string? File { get; set; }
    public int? Line { get; set; }
    public int? Column { get; set; }
}
