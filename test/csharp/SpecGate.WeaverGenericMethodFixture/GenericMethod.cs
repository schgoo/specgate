using SpecGate.Annotations;

namespace SpecGate.WeaverGenericMethodFixture;

/// <summary>
/// A generic method annotated as an operation. Capture resolves the woven
/// method by handle and reads its types from reflection, which an open generic
/// method does not have, so the weaver must reject this declaration.
/// </summary>
public static class GenericMethod
{
    [SpecOperation("echo", Spec = "weaver.negative")]
    public static T Echo<T>([SpecInput("value")] T value) => value;
}
