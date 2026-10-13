using SpecGate.Annotations;

namespace SpecGate.WeaverGenericTypeFixture;

/// <summary>
/// A non-generic method annotated as an operation on a generic type. The
/// declaring type has no single resolvable signature, and discovery cannot
/// publish a type parameter in the registry, so the weaver must reject it.
/// </summary>
/// <typeparam name="T">The unresolved element type.</typeparam>
public static class GenericHolder<T>
{
    [SpecOperation("count", Spec = "weaver.negative")]
    public static int Count([SpecInput("items")] T[] items) => items.Length;
}
