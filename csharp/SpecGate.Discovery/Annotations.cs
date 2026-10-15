using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;

namespace SpecGate.Discovery;

/// <summary>
/// Reads SpecGate annotations from a candidate assembly by type name rather than
/// by compile-time type reference.
/// </summary>
/// <remarks>
/// <para>
/// This tool deliberately does not reference <c>SpecGate.Annotations</c>. The
/// candidate assembly carries its own copy, and a compile-time reference here
/// would make attribute reads depend on the two copies resolving to one
/// <see cref="Type"/> identity. A candidate built against a different annotation
/// version would then silently read as unannotated.
/// </para>
/// <para>
/// Matching on <see cref="Type.FullName"/> removes that coupling entirely, and
/// mirrors how the original reflection program already read
/// <c>SpecEventAttribute</c>. The name is matched against
/// <see cref="CustomAttributeData"/>, so only the annotation itself is ever
/// constructed; unrelated attributes on the same member are left as metadata.
/// </para>
/// </remarks>
internal static class Annotations
{
    private const string Namespace = "SpecGate.Annotations.";

    internal const string Operation = Namespace + "SpecOperationAttribute";
    internal const string Setup = Namespace + "SpecSetupAttribute";
    internal const string Input = Namespace + "SpecInputAttribute";
    internal const string Event = Namespace + "SpecEventAttribute";
    internal const string Exception = Namespace + "SpecExceptionAttribute";

    /// <summary>Open generic <c>Option&lt;T&gt;</c> declared by the annotations package.</summary>
    internal const string OptionType = Namespace + "Option`1";

    /// <summary>Open generic <c>Result&lt;T, E&gt;</c> declared by the annotations package.</summary>
    internal const string ResultType = Namespace + "Result`2";

    /// <summary>
    /// Every attribute on <paramref name="member"/> whose type full name matches
    /// <paramref name="attributeFullName"/>, in reflection order.
    /// </summary>
    /// <remarks>
    /// <paramref name="inherit"/> is supplied per call site because the original
    /// program mixed the two: generic <c>GetCustomAttributes&lt;T&gt;()</c> reads
    /// inherit, while the <c>SpecEvent</c> reads did not.
    /// </remarks>
    internal static object[] On(MemberInfo member, string attributeFullName, bool inherit)
    {
        Type? type = MatchingType(member, attributeFullName, inherit);
        return type is null ? [] : Attribute.GetCustomAttributes(member, type, inherit);
    }

    /// <summary>
    /// Every attribute on <paramref name="parameter"/> whose type full name matches
    /// <paramref name="attributeFullName"/>, in reflection order.
    /// </summary>
    internal static object[] On(ParameterInfo parameter, string attributeFullName, bool inherit)
    {
        Type? type = MatchingType(parameter, attributeFullName, inherit);
        return type is null ? [] : Attribute.GetCustomAttributes(parameter, type, inherit);
    }

    /// <summary>The first matching attribute on <paramref name="member"/>, or <see langword="null"/>.</summary>
    internal static object? FirstOn(MemberInfo member, string attributeFullName, bool inherit) =>
        On(member, attributeFullName, inherit).FirstOrDefault();

    /// <summary>The first matching attribute on <paramref name="parameter"/>, or <see langword="null"/>.</summary>
    internal static object? FirstOn(ParameterInfo parameter, string attributeFullName, bool inherit) =>
        On(parameter, attributeFullName, inherit).FirstOrDefault();

    /// <summary>Whether <paramref name="member"/> carries at least one matching attribute.</summary>
    internal static bool Has(MemberInfo member, string attributeFullName, bool inherit) =>
        FirstOn(member, attributeFullName, inherit) is not null;

    /// <summary>Reads a <see cref="string"/> property from an annotation instance.</summary>
    internal static string? Text(object? attribute, string property) =>
        attribute?.GetType().GetProperty(property)?.GetValue(attribute) as string;

    /// <summary>Reads a <see cref="Type"/> array property from an annotation instance.</summary>
    internal static Type[] Types(object? attribute, string property) =>
        attribute?.GetType().GetProperty(property)?.GetValue(attribute) as Type[] ?? Array.Empty<Type>();

    /// <summary>
    /// Whether <paramref name="definition"/> is the named open generic declared by
    /// the annotations package.
    /// </summary>
    internal static bool IsGenericDefinition(Type definition, string fullName) =>
        string.Equals(definition.FullName, fullName, StringComparison.Ordinal);

    /// <summary>
    /// The annotation <see cref="Type"/> named by <paramref name="attributeFullName"/>
    /// as the candidate assembly declares it, or <see langword="null"/> when the
    /// member carries no such annotation.
    /// </summary>
    /// <remarks>
    /// Metadata is read through <see cref="CustomAttributeData"/>, which names
    /// attribute types without constructing them, so an unrelated attribute whose
    /// constructor throws or whose assembly is unresolvable cannot abort discovery.
    /// Only the resolved annotation type is then materialized, through the
    /// inheritance-aware <see cref="Attribute"/> API.
    /// </remarks>
    private static Type? MatchingType(MemberInfo member, string attributeFullName, bool inherit) =>
        Declarations(member, inherit)
            .SelectMany(declaration => declaration.GetCustomAttributesData())
            .Select(data => data.AttributeType)
            .FirstOrDefault(type => string.Equals(type.FullName, attributeFullName, StringComparison.Ordinal));

    /// <summary>Overload of <see cref="MatchingType(MemberInfo, string, bool)"/> for a parameter.</summary>
    private static Type? MatchingType(ParameterInfo parameter, string attributeFullName, bool inherit) =>
        Declarations(parameter, inherit)
            .SelectMany(declaration => declaration.GetCustomAttributesData())
            .Select(data => data.AttributeType)
            .FirstOrDefault(type => string.Equals(type.FullName, attributeFullName, StringComparison.Ordinal));

    /// <summary>
    /// The members whose own metadata can name an annotation that an inheriting
    /// read of <paramref name="member"/> would return.
    /// </summary>
    private static IEnumerable<MemberInfo> Declarations(MemberInfo member, bool inherit)
    {
        yield return member;
        if (!inherit) yield break;
        if (member is MethodInfo method)
        {
            MethodInfo baseDefinition = method.GetBaseDefinition();
            if (baseDefinition != method) yield return baseDefinition;
        }
        else if (member is Type type)
        {
            for (Type? current = type.BaseType; current is not null; current = current.BaseType) yield return current;
        }
    }

    /// <summary>Overload of <see cref="Declarations(MemberInfo, bool)"/> for a parameter.</summary>
    private static IEnumerable<ParameterInfo> Declarations(ParameterInfo parameter, bool inherit)
    {
        yield return parameter;
        if (!inherit || parameter.Member is not MethodInfo method) yield break;
        MethodInfo baseDefinition = method.GetBaseDefinition();
        if (baseDefinition == method) yield break;
        ParameterInfo[] baseParameters = baseDefinition.GetParameters();
        if (parameter.Position >= 0 && parameter.Position < baseParameters.Length) yield return baseParameters[parameter.Position];
    }
}
