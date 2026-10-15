using System;
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
/// <c>SpecEventAttribute</c>.
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
    internal static object[] On(MemberInfo member, string attributeFullName, bool inherit) =>
        [.. member.GetCustomAttributes(inherit).Where(a => Matches(a, attributeFullName))];

    /// <summary>
    /// Every attribute on <paramref name="parameter"/> whose type full name matches
    /// <paramref name="attributeFullName"/>, in reflection order.
    /// </summary>
    internal static object[] On(ParameterInfo parameter, string attributeFullName, bool inherit) =>
        [.. parameter.GetCustomAttributes(inherit).Where(a => Matches(a, attributeFullName))];

    /// <summary>The first matching attribute on <paramref name="member"/>, or <see langword="null"/>.</summary>
    internal static object? FirstOn(MemberInfo member, string attributeFullName, bool inherit) =>
        member.GetCustomAttributes(inherit).FirstOrDefault(a => Matches(a, attributeFullName));

    /// <summary>The first matching attribute on <paramref name="parameter"/>, or <see langword="null"/>.</summary>
    internal static object? FirstOn(ParameterInfo parameter, string attributeFullName, bool inherit) =>
        parameter.GetCustomAttributes(inherit).FirstOrDefault(a => Matches(a, attributeFullName));

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

    private static bool Matches(object attribute, string attributeFullName) =>
        string.Equals(attribute.GetType().FullName, attributeFullName, StringComparison.Ordinal);
}
