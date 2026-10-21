using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;

namespace SpecGate.Discovery;

/// <summary>A method carrying at least one SpecGate operation or setup annotation.</summary>
internal sealed record AnnotatedMethod(MethodInfo Method, object[] Operations, object[] Setups);

/// <summary>
/// Builds the discovery payload for a single requested component: its operations
/// and the annotated types those operations reference.
/// </summary>
internal sealed class ComponentDocument
{
    private readonly string component;
    private readonly IReadOnlyList<Type> allTypes;
    private readonly IReadOnlyList<AnnotatedMethod> annotatedMethods;
    private readonly NullabilityInfoContext nullability;
    private readonly SemanticTypes semantic;

    internal ComponentDocument(
        string component,
        IReadOnlyList<Type> allTypes,
        IReadOnlyList<AnnotatedMethod> annotatedMethods,
        NullabilityInfoContext nullability)
    {
        this.component = component;
        this.allTypes = allTypes;
        this.annotatedMethods = annotatedMethods;
        this.nullability = nullability;
        semantic = new SemanticTypes(nullability);
    }

    /// <summary>Builds the serializable payload for this component.</summary>
    internal object Build()
    {
        var operations = new List<object>();

        HashSet<string> selectedOperationNames = annotatedMethods
            .SelectMany(entry => entry.Operations)
            .Where(operation => Annotations.Text(operation, "Spec") == component)
            .Select(operation => Annotations.Text(operation, "Name") ?? string.Empty)
            .ToHashSet(StringComparer.Ordinal);

        foreach (AnnotatedMethod annotated in annotatedMethods)
        {
            MethodInfo method = annotated.Method;

            foreach (object operation in annotated.Operations)
            {
                if (Annotations.Text(operation, "Spec") != component) continue;
                operations.Add(BuildOperation(method, operation));
            }

            foreach (object setup in annotated.Setups)
            {
                // An explicitly scoped setup belongs to exactly its own component.
                // An unscoped setup belongs to whichever component declares the
                // operation it prepares; without that guard it would leak into
                // every requested component's document.
                string? scope = Annotations.Text(setup, "Spec");
                string name = Annotations.Text(setup, "Name") ?? string.Empty;
                if (scope is not null)
                {
                    if (scope != component) continue;
                }
                else if (!selectedOperationNames.Contains(name))
                {
                    continue;
                }

                operations.Add(BuildSetup(method, setup, name, selectedOperationNames));
            }
        }

        return new { operations, types = BuildTypes() };
    }

    private object BuildOperation(MethodInfo method, object operation)
    {
        Type[] arguments = Annotations.Types(operation, "TypeArguments");
        if (arguments.Length != method.GetGenericArguments().Length || arguments.Any(type => type.ContainsGenericParameters))
        {
            throw new InvalidOperationException(
                $"{method} requires one concrete TypeArguments entry per generic method parameter.");
        }

        if (method.IsGenericMethodDefinition)
        {
            method = method.MakeGenericMethod(arguments);
        }

        NullabilityInfo returnInfo = nullability.Create(method.ReturnParameter);
        (bool isAsync, Type? inner, NullabilityInfo? innerInfo) = Unwrap(method.ReturnType, returnInfo);
        bool hasException = Annotations.Has(method, Annotations.Exception, inherit: true);

        string output;
        if (inner is null)
        {
            output = hasException ? "Result<(), string>" : "";
        }
        else
        {
            string mapped = semantic.Render(inner, innerInfo);
            output = hasException ? "Result<" + mapped + ", string>" : mapped;
        }

        object? exception = Annotations.FirstOn(method, Annotations.Exception, inherit: true);
        object? csExceptions = exception is null
            ? null
            : Annotations.Types(exception, "ExceptionTypes").Select(type => type.Name).ToArray();

        return new
        {
            name = Annotations.Text(operation, "Name") ?? string.Empty,
            is_setup = false,
            is_async = isAsync,
            is_method = !method.IsStatic,
            is_public = method.IsPublic,
            return_type = output,
            fills = "",
            @params = BuildParams(method),
            component,
            cs_class = SourceTypeName(method.DeclaringType),
            cs_method_of = method.DeclaringType?.Name ?? "",
            cs_method = method.Name,
            cs_is_static = method.IsStatic,
            cs_return = RawTypeNames.Render(method.ReturnType, returnInfo),
            cs_params = BuildRawParams(method),
            cs_exceptions = csExceptions,
        };
    }

    private object BuildSetup(
        MethodInfo method,
        object setup,
        string name,
        HashSet<string> selectedOperationNames)
    {
        NullabilityInfo returnInfo = nullability.Create(method.ReturnParameter);
        (bool isAsync, Type? inner, NullabilityInfo? innerInfo) = Unwrap(method.ReturnType, returnInfo);

        string returnType;
        string csReturn;
        using (semantic.SuspendCollection())
        {
            returnType = inner is null ? "" : semantic.Render(inner, innerInfo);
            csReturn = RawTypeNames.Render(method.ReturnType, returnInfo);
        }

        if (selectedOperationNames.Contains(name)
            && inner is not null
            && Annotations.Has(inner, Annotations.Event, inherit: false))
        {
            semantic.Collect(inner);
        }

        return new
        {
            name,
            is_setup = true,
            is_async = isAsync,
            is_method = !method.IsStatic,
            is_public = method.IsPublic,
            return_type = returnType,
            fills = Annotations.Text(setup, "Fills") ?? "",
            @params = BuildParams(method),
            component,
            cs_class = SourceTypeName(method.DeclaringType),
            cs_method_of = method.DeclaringType?.Name ?? "",
            cs_method = method.Name,
            cs_is_static = method.IsStatic,
            cs_return = csReturn,
            cs_params = BuildRawParams(method),
            cs_exceptions = (object?)null,
        };
    }

    private List<object> BuildTypes()
    {
        var records = new List<object>();

        // The queue grows while it is walked: publishing a type renders its
        // members, which can reach further annotated types.
        for (int i = 0; i < semantic.Queue.Count; i++)
        {
            Type type = semantic.Queue[i];
            string name = SemanticTypes.NameOf(type);

            if (type.IsEnum)
            {
                records.Add(new
                {
                    name,
                    kind = "enum",
                    fields = new List<string[]>(),
                    variants = EnumVariants(type),
                    component,
                });
            }
            else if (type.IsAbstract)
            {
                var variants = allTypes
                    .Where(candidate => candidate != type && !candidate.IsAbstract && type.IsAssignableFrom(candidate))
                    .Select(subtype => (object)new { name = SemanticTypes.NameOf(subtype), fields = SpecMembers(subtype) })
                    .ToList();

                records.Add(new
                {
                    name,
                    kind = "enum",
                    fields = new List<string[]>(),
                    variants,
                    component,
                });
            }
            else
            {
                records.Add(new
                {
                    name,
                    kind = "struct",
                    fields = SpecMembers(type),
                    variants = new List<object>(),
                    component,
                });
            }
        }

        return records;
    }

    /// <summary>
    /// Enum variants in declaration order.
    /// </summary>
    /// <remarks>
    /// Declaration order, not numeric order: <c>Enum.GetNames</c> orders by
    /// underlying value, which would reorder the published variants away from the
    /// equivalent Rust enum. A plain enum's variants carry no payload, so each one
    /// publishes an empty field list.
    /// </remarks>
    private static List<object> EnumVariants(Type type)
    {
        var variants = new List<object>();
        foreach (FieldInfo field in type.GetFields(BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly))
        {
            if (field.IsSpecialName) continue;
            variants.Add(new { name = SemanticTypes.NameOf(field), fields = new List<string[]>() });
        }

        return variants;
    }

    private List<string[]> SpecMembers(Type type)
    {
        var members = new List<string[]>();

        foreach (PropertyInfo property in type.GetProperties(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly))
        {
            if (!Annotations.Has(property, Annotations.Event, inherit: false)) continue;
            members.Add([SemanticTypes.NameOf(property), semantic.Render(property.PropertyType, nullability.Create(property))]);
        }

        foreach (FieldInfo field in type.GetFields(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly))
        {
            if (!Annotations.Has(field, Annotations.Event, inherit: false)) continue;
            members.Add([SemanticTypes.NameOf(field), semantic.Render(field.FieldType, nullability.Create(field))]);
        }

        return members;
    }

    private List<string[]> BuildParams(MethodInfo method)
    {
        var list = new List<string[]>();
        foreach (ParameterInfo parameter in method.GetParameters())
        {
            list.Add([ParameterName(parameter), semantic.RenderParameter(parameter)]);
        }

        return list;
    }

    private List<string[]> BuildRawParams(MethodInfo method)
    {
        var list = new List<string[]>();
        foreach (ParameterInfo parameter in method.GetParameters())
        {
            list.Add([ParameterName(parameter), RawTypeNames.Render(parameter.ParameterType, nullability.Create(parameter))]);
        }

        return list;
    }

    private static string ParameterName(ParameterInfo parameter) =>
        Annotations.Text(Annotations.FirstOn(parameter, Annotations.Input, inherit: true), "Name")
        ?? parameter.Name
        ?? "";

    private static (bool IsAsync, Type? Inner, NullabilityInfo? InnerInfo) Unwrap(Type returnType, NullabilityInfo? returnInfo)
    {
        if (returnType == typeof(void)) return (false, null, null);
        if (returnType == typeof(System.Threading.Tasks.Task) || returnType == typeof(System.Threading.Tasks.ValueTask))
        {
            return (true, null, null);
        }

        if (returnType.IsGenericType)
        {
            Type definition = returnType.GetGenericTypeDefinition();
            if (definition == typeof(System.Threading.Tasks.Task<>) || definition == typeof(System.Threading.Tasks.ValueTask<>))
            {
                Type inner = returnType.GetGenericArguments()[0];
                NullabilityInfo? innerInfo =
                    returnInfo?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
                return (true, inner, innerInfo);
            }
        }

        return (false, returnType, returnInfo);
    }

    /// <summary>
    /// Reflection spells nested CLR types with <c>+</c>, while generated metadata
    /// is replayed as C# source names, whose nested separator is <c>.</c>.
    /// </summary>
    private static string SourceTypeName(Type? type) =>
        (type?.FullName ?? type?.Name ?? "").Replace('+', '.');
}
